//! Dual-backend project assembly for tpt-telos (Phase 3).
//!
//! [`generate_project`] routes each module to the Rust or Go backend, generates
//! the corresponding sources, and -- when a program spans *both* backends --
//! emits the automatic FFI bridge so the two halves can call each other with no
//! hand-written glue. The result is a ready-to-build project tree:
//!
//! ```text
//! <out>/
//!   rust/Cargo.toml
//!   rust/src/lib.rs
//!   rust/src/ffi.rs      (dual-backend only)
//!   go/go.mod
//!   go/service.go
//!   go/ffi.go            (dual-backend only)
//!   go/telos_ffi.h       (dual-backend only)
//! ```

use std::io;
use std::path::Path;

use tpt_telos_agent::FuncOutcome;
use tpt_telos_parser::ast::*;
use tpt_telos_router::{RoutingDiagnostic, Target};

use crate::{collect_bodies, ffi, go, python, render_rust};

/// Go package name used for the generated service and its FFI shims.
pub const GO_PACKAGE: &str = "gosvc";

/// Crate name used for the generated Rust backend library and its compiled
/// artifacts (`lib<RUST_CRATE_NAME>.rlib` / `lib<RUST_CRATE_NAME>.a`). Promoted
/// to a `pub const` so consumers (e.g. `tpt-telos-sdk`) can locate the artifact
/// bytes without hardcoding the string.
pub const RUST_CRATE_NAME: &str = "generated_rust";

/// A single generated file, addressed by a path relative to the project root.
///
/// # Examples
///
/// A `GeneratedFile` is produced by [`generate_project`]; you typically inspect
/// it by path:
///
/// ```
/// use tpt_telos_parser::parse;
/// use tpt_telos_agent::{StaticAgent, transpile_module};
/// use tpt_telos_codegen::generate_project;
///
/// let src = r#"
///     module Bank {
///         invariant Wallet { balance >= 0 }
///         func deposit(w: Wallet, amount: PositiveInt)
///             ensures w.balance == old(w.balance) + amount
///         ;
///     }
/// "#;
///
/// let modules = parse(src).unwrap();
/// let outcomes: Vec<_> = modules.iter()
///     .flat_map(|m| transpile_module(m, &StaticAgent::new()).unwrap())
///     .collect();
///
/// let project = generate_project(&modules, &outcomes).unwrap();
/// let lib = project.files.iter().find(|f| f.path == "rust/src/lib.rs").unwrap();
/// assert!(lib.contents.contains("pub struct Wallet"));
/// ```
#[derive(Debug, Clone)]
pub struct GeneratedFile {
    pub path: String,
    pub contents: String,
}

/// A fully assembled dual-backend project.
#[derive(Debug, Clone)]
pub struct Project {
    pub files: Vec<GeneratedFile>,
    pub has_rust: bool,
    pub has_go: bool,
    pub has_ffi: bool,
    pub has_python: bool,
    /// Routing diagnostics (e.g. real_time + Go conflicts) collected during
    /// module routing.  Callers should surface these as warnings.
    pub diagnostics: Vec<RoutingDiagnostic>,
}

impl Project {
    /// Write every generated file under `root`, creating directories as needed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tpt_telos_parser::parse;
    /// use tpt_telos_agent::{StaticAgent, transpile_module};
    /// use tpt_telos_codegen::generate_project;
    ///
    /// let src = r#"
    ///     module Bank {
    ///         invariant Wallet { balance >= 0 }
    ///         func deposit(w: Wallet, amount: PositiveInt)
    ///             ensures w.balance == old(w.balance) + amount
    ///         ;
    ///     }
    /// "#;
    ///
    /// let modules = parse(src).unwrap();
    /// let outcomes: Vec<_> = modules.iter()
    ///     .flat_map(|m| transpile_module(m, &StaticAgent::new()).unwrap())
    ///     .collect();
    ///
    /// let project = generate_project(&modules, &outcomes).unwrap();
    /// project.write(std::path::Path::new("/tmp/my_bank_project")).unwrap();
    /// // Writes rust/src/lib.rs, rust/Cargo.toml, and go/service.go etc.
    /// ```
    pub fn write(&self, root: &Path) -> io::Result<()> {
        for f in &self.files {
            let full = root.join(&f.path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&full, &f.contents)?;
        }
        Ok(())
    }
}

/// Assemble the dual-backend project for a program.
///
/// Returns `Err` if any FFI-boundary parameter type is unsupported (e.g. a
/// float, bool, string, array, or slice).  All-integer programs always succeed.
///
/// # Examples
///
/// ```
/// use tpt_telos_parser::parse;
/// use tpt_telos_agent::{StaticAgent, transpile_module};
/// use tpt_telos_codegen::generate_project;
///
/// let src = r#"
///     module Bank {
///         invariant Wallet { balance >= 0 }
///         func deposit(w: Wallet, amount: PositiveInt)
///             ensures w.balance == old(w.balance) + amount
///         ;
///     }
/// "#;
///
/// let modules = parse(src).unwrap();
/// let outcomes: Vec<_> = modules.iter()
///     .flat_map(|m| transpile_module(m, &StaticAgent::new()).unwrap())
///     .collect();
///
/// let project = generate_project(&modules, &outcomes).unwrap();
/// // A Rust-only program (no @boundary(network_io) etc.) only produces Rust output.
/// assert!(project.has_rust);
/// assert!(!project.has_ffi);
/// ```
pub fn generate_project(modules: &[Module], outcomes: &[FuncOutcome]) -> Result<Project, String> {
    let bodies = collect_bodies(outcomes);

    // Route every module and collect diagnostics.
    let mut all_diagnostics: Vec<RoutingDiagnostic> = Vec::new();
    let mut rust_mods: Vec<&Module> = Vec::new();
    let mut go_mods: Vec<&Module> = Vec::new();
    let mut py_mods: Vec<&Module> = Vec::new();
    for m in modules {
        let (route, diags) = tpt_telos_router::route_checked(&m.attributes, &m.name);
        all_diagnostics.extend(diags);
        match route.target {
            Target::Rust => rust_mods.push(m),
            Target::Go => go_mods.push(m),
            Target::Python => py_mods.push(m),
        }
    }

    let has_rust = !rust_mods.is_empty();
    let has_go = !go_mods.is_empty();
    let has_python = !py_mods.is_empty();
    let has_ffi = has_rust && has_go;

    let mut files = Vec::new();

    if has_rust {
        let mut lib = render_rust(&rust_mods, &bodies);
        if has_ffi {
            lib.push_str("\npub mod ffi;\n");
        }
        files.push(GeneratedFile {
            path: "rust/src/lib.rs".to_string(),
            contents: lib,
        });
        files.push(GeneratedFile {
            path: "rust/Cargo.toml".to_string(),
            contents: rust_cargo_toml(has_ffi),
        });
    }

    if has_go {
        let service = go::generate_go_package(&go_mods, &bodies, GO_PACKAGE);
        files.push(GeneratedFile {
            path: "go/service.go".to_string(),
            contents: service,
        });
        files.push(GeneratedFile {
            path: "go/go.mod".to_string(),
            contents: go_mod(),
        });
    }

    if has_ffi {
        let bridge = ffi::generate_bridge(modules, &bodies, GO_PACKAGE)?;
        files.push(GeneratedFile {
            path: "rust/src/ffi.rs".to_string(),
            contents: bridge.rust,
        });
        files.push(GeneratedFile {
            path: "go/ffi.go".to_string(),
            contents: bridge.go,
        });
        files.push(GeneratedFile {
            path: "go/telos_ffi.h".to_string(),
            contents: bridge.header,
        });
    }

    if has_python {
        // Detect JAX flag for any Python-target module.
        let use_jax = py_mods.iter().any(|m| {
            m.attributes.iter().any(|a| {
                a.name == "boundary"
                    && a.args
                        .iter()
                        .any(|arg| matches!(arg, tpt_telos_parser::ast::Arg::Flag(f) if f == "jax"))
            })
        });
        let service = python::generate_python_package(&py_mods, &bodies, use_jax);
        files.push(GeneratedFile {
            path: "python/service.py".to_string(),
            contents: service,
        });
        let req = if use_jax {
            "jax\n"
        } else {
            "# No runtime dependencies\n"
        };
        files.push(GeneratedFile {
            path: "python/requirements.txt".to_string(),
            contents: req.to_string(),
        });
    }

    Ok(Project {
        files,
        has_rust,
        has_go,
        has_ffi,
        has_python,
        diagnostics: all_diagnostics,
    })
}

fn rust_cargo_toml(dual: bool) -> String {
    // A dual-backend crate is built as a staticlib (for linking into the Go
    // binary) plus an rlib (so `cargo build` type-checks standalone).
    let lib_section = if dual {
        "\n[lib]\ncrate-type = [\"staticlib\", \"rlib\"]\n"
    } else {
        ""
    };
    format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{lib}\n[dependencies]\n\n[workspace]\n",
        RUST_CRATE_NAME,
        lib = lib_section
    )
}

fn go_mod() -> String {
    format!("module telos/{}\n\ngo 1.21\n", GO_PACKAGE)
}
