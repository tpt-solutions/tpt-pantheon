//! tpt-telos command-line interface.
//!
//! Usage:
//!   telos parse     <file.telos>   pretty-print the parsed AST
//!   telos verify    <file.telos>   run formal verification and report pass/fail
//!   telos transpile <file.telos>   run the agentic transpiler and emit Rust
//!   telos build     <file.telos>   transpile then compile the generated Rust
//!   telos project   <file.telos>   generate a dual-backend (Rust + Go) project
//!                                   with an automatic FFI bridge
//!   telos eject     <file.telos>   eject functions to raw Rust/Go opaque blocks
//!                                   guarded by their contracts
//!   telos lsp                       run the language server (LSP over stdio)

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use serde::Serialize;
use std::fs;
use std::process::ExitCode;

use tpt_telos_agent::{transpile_module, StaticAgent};
use tpt_telos_codegen::{generate_program, generate_project, proof};
use tpt_telos_parser::ast::*;
use tpt_telos_parser::parse;
use tpt_telos_verifier::verify;

#[cfg(feature = "llm")]
use tpt_telos_agent::llm_agent::LlmAgent;

#[derive(Parser)]
#[command(
    name = "telos",
    version,
    about = "tpt-telos compiler frontend (Phase 4)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scaffold a starter .telos module file.
    Init {
        /// Name of the module to generate (default: "MyModule").
        #[arg(long, default_value = "MyModule")]
        module: String,
        /// Output file path (default: <module>.telos).
        #[arg(long)]
        out: Option<String>,
        /// Starter template to scaffold. Options: simple (default), dual-backend,
        /// eject, real-time, python-ml, cross-module.
        #[arg(long, default_value = "simple")]
        template: String,
    },
    /// Scaffold a project directory (a .telos module plus a README) that can be
    /// passed straight to `telos project --check`.
    New {
        /// Name of the project (and module) to generate (default: "MyProject").
        #[arg(long, default_value = "MyProject")]
        name: String,
        /// Output directory for the project (default: `<name>`).
        #[arg(long)]
        out_dir: Option<String>,
        /// Starter template to scaffold. Options: simple (default), dual-backend,
        /// eject, real-time, python-ml, cross-module.
        #[arg(long, default_value = "simple")]
        template: String,
    },
    /// Parse a .telos file and print its AST.
    Parse {
        /// Path to the .telos source file.
        file: String,
        /// Emit machine-readable JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
    },
    /// Run formal verification on a .telos file (pass/fail report).
    Verify {
        /// Path to the .telos source file.
        file: String,
        /// Solver backend to use. Default: fourier-motzkin (built-in).
        /// Use `z3` for exact nonlinear arithmetic (requires the `z3` feature).
        #[arg(long, default_value = "fourier-motzkin")]
        solver: String,
        /// Emit machine-readable JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
        /// Re-run verification automatically when the source file changes.
        #[arg(long)]
        watch: bool,
    },
    /// Run the agentic transpiler and print generated Rust.
    Transpile {
        /// Path to the .telos source file.
        file: String,
        /// Use the LLM-backed agent instead of the offline static synthesizer.
        #[arg(long)]
        llm: bool,
        /// Write the generated Rust to this path instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Transpile then compile the generated Rust (requires cargo/rustc).
    Build {
        /// Path to the .telos source file.
        file: String,
        /// Emit the generated crate into this directory.
        #[arg(long, default_value = "gen")]
        out_dir: String,
        /// Use the LLM-backed agent instead of the offline static synthesizer.
        #[arg(long)]
        llm: bool,
        /// Solver backend to use.
        #[arg(long, default_value = "fourier-motzkin")]
        solver: String,
        /// Emit machine-readable JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
        /// Re-run the build automatically when the source file (or any .telos
        /// file in its directory) changes.
        #[arg(long)]
        watch: bool,
    },
    /// Generate a dual-backend project (Rust + Go) with an automatic FFI bridge.
    Project {
        /// Path to the .telos source file.
        file: String,
        /// Emit the generated project into this directory.
        #[arg(long, default_value = "gen-project")]
        out_dir: String,
        /// Use the LLM-backed agent instead of the offline static synthesizer.
        #[arg(long)]
        llm: bool,
        /// After generating, compile the Rust crate (cargo) and vet the Go
        /// package (go) to prove both backends build.
        #[arg(long)]
        check: bool,
        /// Exit non-zero if any module has a real_time or zero_allocation
        /// routing conflict with Go (GC is non-deterministic).
        #[arg(long)]
        strict_rt: bool,
        /// Emit machine-readable JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
        /// Re-run the project generation/check automatically when the source
        /// file (or any .telos file in its directory) changes.
        #[arg(long)]
        watch: bool,
    },
    /// Eject functions to raw Rust/Go opaque blocks guarded by their contracts.
    Eject {
        /// Path to the .telos source file.
        file: String,
        /// Emit the ejected project into this directory.
        #[arg(long, default_value = "ejected")]
        out_dir: String,
        /// Eject only this function (by name). Default: eject every function.
        #[arg(long)]
        func: Option<String>,
        /// Use the LLM-backed agent instead of the offline static synthesizer.
        #[arg(long)]
        llm: bool,
        /// Exit non-zero if any module has a real_time or zero_allocation
        /// routing conflict with Go (GC is non-deterministic).
        #[arg(long)]
        strict_rt: bool,
        /// Emit machine-readable JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
    },
    /// Verify a proof manifest against the current source file.
    VerifyManifest {
        /// Path to the telos-proof.json manifest.
        manifest: String,
        /// Path to the original .telos source file.
        source: String,
    },
    /// Print shell completions for the given shell and exit.
    Completions {
        /// Shell to generate completions for (bash, zsh, fish, powershell, elvish).
        shell: Shell,
    },
    /// Reformat a .telos file canonically (reuses the LSP's formatter).
    Fmt {
        /// Path to the .telos source file.
        file: String,
        /// Check formatting without writing; exit non-zero if changes are needed.
        #[arg(long)]
        check: bool,
        /// Print the formatted output to stdout instead of writing it back.
        #[arg(long)]
        stdout: bool,
    },
    /// Check for optional external tools (go, gofmt, Z3) needed by some commands.
    Doctor {
        /// Emit machine-readable JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
    },
    /// Run the tpt-telos language server (LSP over stdio) for IDE integration.
    Lsp,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Init {
            module,
            out,
            template,
        } => match run_init(&module, out.as_deref(), &template) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("init error: {e}");
                ExitCode::FAILURE
            }
        },
        Command::New {
            name,
            out_dir,
            template,
        } => match run_new(&name, out_dir.as_deref(), &template) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("new error: {e}");
                ExitCode::FAILURE
            }
        },
        Command::Parse { file, json } => match run_parse(&file, json) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("parse error: {e}");
                ExitCode::FAILURE
            }
        },
        Command::Verify {
            file,
            solver,
            json,
            watch,
        } => match run_verify(&file, &solver, json, watch) {
            Ok(passed) => {
                if passed {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("verify error: {e}");
                ExitCode::FAILURE
            }
        },
        Command::Transpile { file, llm, out } => match run_transpile(&file, llm, out) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("transpile error: {e}");
                ExitCode::FAILURE
            }
        },
        Command::Build {
            file,
            out_dir,
            llm,
            solver,
            json,
            watch,
        } => match run_build(&file, &out_dir, llm, &solver, json, watch) {
            Ok(passed) => {
                if passed {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("build error: {e}");
                ExitCode::FAILURE
            }
        },
        Command::Project {
            file,
            out_dir,
            llm,
            check,
            strict_rt,
            json,
            watch,
        } => match run_project(&file, &out_dir, llm, check, strict_rt, json, watch) {
            Ok(passed) => {
                if passed {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("project error: {e}");
                ExitCode::FAILURE
            }
        },
        Command::Eject {
            file,
            out_dir,
            func,
            llm,
            strict_rt,
            json,
        } => match run_eject(&file, &out_dir, func.as_deref(), llm, strict_rt, json) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("eject error: {e}");
                ExitCode::FAILURE
            }
        },
        Command::VerifyManifest { manifest, source } => {
            match run_verify_manifest(&manifest, &source) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("verify-manifest error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "telos", &mut std::io::stdout());
            ExitCode::SUCCESS
        }
        Command::Fmt {
            file,
            check,
            stdout,
        } => match run_fmt(&file, check, stdout) {
            Ok(ok) => {
                if ok {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("fmt error: {e}");
                ExitCode::FAILURE
            }
        },
        Command::Doctor { json } => {
            run_doctor(json);
            ExitCode::SUCCESS
        }
        Command::Lsp => match tpt_telos_lsp::run_stdio() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("lsp error: {e}");
                ExitCode::FAILURE
            }
        },
    }
}

fn load_modules(file: &str) -> Result<Vec<Module>, String> {
    let src = fs::read_to_string(file).map_err(|e| format!("cannot read `{file}`: {e}"))?;
    parse(&src)
}

/// Reformat `file` canonically, reusing the same `format_source` the LSP's
/// `textDocument/formatting` handler calls. Returns `true` when the file is
/// (or was already) in canonical form, `false` when `--check` found changes
/// needed — the caller maps that to the process exit code.
fn run_fmt(file: &str, check: bool, stdout: bool) -> Result<bool, String> {
    let src = fs::read_to_string(file).map_err(|e| format!("cannot read `{file}`: {e}"))?;
    let formatted = tpt_telos_lsp::analysis::format_source(&src)?;
    let unchanged = formatted == src;

    if stdout {
        print!("{formatted}");
        return Ok(true);
    }

    if check {
        if unchanged {
            println!("{file}: already formatted");
        } else {
            println!("{file}: would reformat");
        }
        return Ok(unchanged);
    }

    if unchanged {
        println!("{file}: already formatted");
    } else {
        fs::write(file, &formatted).map_err(|e| format!("cannot write `{file}`: {e}"))?;
        println!("Reformatted {file}");
    }
    Ok(true)
}

#[derive(Serialize)]
struct DoctorCheck {
    tool: String,
    found: bool,
    version: Option<String>,
    needed_for: String,
}

/// Probe `cmd --version-flag` and report whether it ran successfully, along
/// with the first line of its output as a version string.
fn probe_tool(cmd: &str, version_flag: &str) -> (bool, Option<String>) {
    match std::process::Command::new(cmd).arg(version_flag).output() {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let first_line = text
                .lines()
                .next()
                .filter(|l| !l.is_empty())
                .map(|l| l.trim().to_string());
            (true, first_line)
        }
        _ => (false, None),
    }
}

#[cfg(feature = "z3")]
fn z3_doctor_check() -> DoctorCheck {
    DoctorCheck {
        tool: "z3".to_string(),
        found: tpt_telos_verifier::z3_solver::is_z3_available(),
        version: None,
        needed_for: "`telos verify --solver z3` (exact nonlinear arithmetic)".to_string(),
    }
}

#[cfg(not(feature = "z3"))]
fn z3_doctor_check() -> DoctorCheck {
    DoctorCheck {
        tool: "z3".to_string(),
        found: false,
        version: Some("not applicable: build with `--features z3` to enable".to_string()),
        needed_for: "`telos verify --solver z3` (exact nonlinear arithmetic)".to_string(),
    }
}

/// Check for the optional external tools some commands shell out to
/// (`project --check`, `eject`'s Go backend, `--solver z3`) and report
/// what's available, so missing tools are diagnosed upfront instead of
/// surfacing as a confusing failure deep into a build.
fn run_doctor(json: bool) {
    let mut checks = Vec::new();

    let (go_found, go_version) = probe_tool("go", "version");
    checks.push(DoctorCheck {
        tool: "go".to_string(),
        found: go_found,
        version: go_version,
        needed_for: "`telos project --check` / `telos eject` (Go backend)".to_string(),
    });

    let (gofmt_found, gofmt_version) = probe_tool("gofmt", "-h");
    checks.push(DoctorCheck {
        tool: "gofmt".to_string(),
        found: gofmt_found,
        version: gofmt_version,
        needed_for: "canonicalizing generated Go (`project`/`eject`)".to_string(),
    });

    checks.push(z3_doctor_check());

    if json {
        println!("{}", serde_json::to_string_pretty(&checks).unwrap());
        return;
    }

    println!("telos doctor: optional external tool check\n");
    for c in &checks {
        let status = if c.found { "found" } else { "missing" };
        let detail = c.version.as_deref().unwrap_or("");
        println!("  [{status:>7}] {:<8} {}", c.tool, detail);
        println!("            needed for: {}", c.needed_for);
    }
}

// ---------------------------------------------------------------------------
// Colorized / rustc-style diagnostic output
// ---------------------------------------------------------------------------

const RESET: &str = "\x1b[0m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";

/// Whether to emit ANSI colors. Honors `NO_COLOR` and disables color when
/// stdout is not a terminal (so piped / file output stays plain text).
fn use_color() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    std::io::IsTerminal::is_terminal(&std::io::stdout())
}

/// Wrap `s` in an ANSI color escape when `enabled`, otherwise return it untouched.
fn paint(enabled: bool, color: &str, s: &str) -> String {
    if enabled {
        format!("{}{}{}", color, s, RESET)
    } else {
        s.to_string()
    }
}

/// Print a rustc-style source annotation (`--> file:line:col`, the offending
/// line, and a caret) for a failed check's source span `(line, column)`.
fn emit_caret(enabled: bool, file: &str, lines: &[&str], span: (usize, usize)) {
    let (line, col) = span;
    if line == 0 || line > lines.len() {
        return;
    }
    let src_line = lines[line - 1];
    let w = line.to_string().len();
    println!("{} {}:{}:{}", paint(enabled, CYAN, "-->"), file, line, col);
    println!("{} |", " ".repeat(w));
    println!("{:>width$} | {}", line, src_line, width = w);
    let pad = " ".repeat(col.saturating_sub(1));
    println!("{} | {}{}", " ".repeat(w), pad, paint(enabled, RED, "^"));
}

/// Latest modification time among `.telos` files in `dir`, used by watch mode
/// to detect changes across the whole directory (not just the entry file).
fn latest_telos_mtime(dir: &std::path::Path) -> Option<std::time::SystemTime> {
    let mut max: Option<std::time::SystemTime> = None;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("telos") {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(m) = meta.modified() {
                        max = Some(match max {
                            Some(cur) => cur.max(m),
                            None => m,
                        });
                    }
                }
            }
        }
    }
    max
}

/// Poll `file`'s directory for `.telos` changes and re-run `run` on each
/// change after a short debounce window. Runs `run` once before entering the
/// watch loop so the initial result is reported immediately.
fn watch_loop<F>(file: &str, run: F) -> Result<bool, String>
where
    F: Fn() -> Result<bool, String>,
{
    let dir = std::path::Path::new(file)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut last_modified = latest_telos_mtime(&dir);
    // Initial run; surface any fatal error before entering the watch loop.
    run()?;
    eprintln!("Watching {} for changes (Ctrl+C to stop)...", dir.display());
    // Track when the directory last became stable so we only re-run after the
    // source has stopped changing for the debounce window (avoids re-running
    // on every keystroke of a multi-file save).
    let mut stable_since: Option<std::time::Instant> = None;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(250));
        let current = latest_telos_mtime(&dir);
        if current != last_modified {
            last_modified = current;
            stable_since = Some(std::time::Instant::now());
            continue;
        }
        if let Some(since) = stable_since {
            if since.elapsed() >= std::time::Duration::from_millis(400) {
                stable_since = None;
                println!("\n--- change detected, re-running ---\n");
                if let Err(e) = run() {
                    eprintln!("error: {e}");
                }
            }
        }
    }
}

/// Canonicalise generated Go with `gofmt -w`. Go's printer tightens operator
/// spacing by precedence (e.g. `a == b+c`), which a string-based generator
/// cannot easily reproduce, so we defer final formatting to the real tool.
/// No-ops (with a note) when gofmt is not installed.
fn canonicalize_go(go_dir: &std::path::Path) {
    let out = std::process::Command::new("gofmt")
        .arg("-w")
        .arg(".")
        .current_dir(go_dir)
        .output();
    match out {
        Ok(o) if o.status.success() => {}
        Ok(o) => eprintln!(
            "note: gofmt could not format generated Go: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(_) => eprintln!("note: gofmt not found; generated Go left un-formatted."),
    }
}

fn run_init(module_name: &str, out: Option<&str>, template: &str) -> Result<(), String> {
    let default_path = format!("{}.telos", module_name);
    let path = out.unwrap_or(&default_path);
    let content = render_init_template(module_name, template)?;
    fs::write(path, &content).map_err(|e| format!("cannot write `{path}`: {e}"))?;
    println!("Scaffolded {path} with module `{module_name}` (template: {template}).");
    println!("Run `telos verify {path}` to verify the contracts.");
    Ok(())
}

/// Scaffold a project directory: a `<name>.telos` module (from `template`)
/// plus a `README.md` with the next steps, so a beginner can go from zero to
/// `telos project --check` in one command.
fn run_new(name: &str, out_dir: Option<&str>, template: &str) -> Result<(), String> {
    let root = std::path::Path::new(out_dir.unwrap_or(name));
    let telos_path = root.join(format!("{name}.telos"));
    let readme_path = root.join("README.md");

    fs::create_dir_all(root).map_err(|e| format!("cannot create `{root:?}`: {e}"))?;

    let content = render_init_template(name, template)?;
    fs::write(&telos_path, &content).map_err(|e| format!("cannot write `{telos_path:?}`: {e}"))?;

    let readme = format!(
        "# {name}\n\n\
         A tpt-telos project scaffolded with `telos new` (template: {template}).\n\n\
         ## Next steps\n\n\
         - Verify the contracts: `telos verify {name}.telos`\n\
         - Build a verified Rust crate: `telos build {name}.telos --out-dir gen`\n\
         - Generate a dual Rust+Go project: `telos project {name}.telos --out-dir gen-project --check`\n\
         - Emit machine-readable output for CI: `telos verify {name}.telos --json`\n"
    );
    fs::write(&readme_path, &readme).map_err(|e| format!("cannot write `{readme_path:?}`: {e}"))?;

    println!("Scaffolded project in `{root:?}` (template: {template}).");
    println!("  - {telos_path:?}");
    println!("  - {readme_path:?}");
    println!("Run `telos project {telos_path:?} --out-dir gen-project --check` to build it.");
    Ok(())
}

/// Render the source of a starter `.telos` module for `telos init` / `telos new`.
fn render_init_template(module_name: &str, template: &str) -> Result<String, String> {
    let content = match template {
        "simple" | "" => format!(
            "@boundary(cpu_bound)\n\
             module {mod} {{\n\
             \n\
                 invariant Counter {{\n\
                     count >= 0\n\
                 }}\n\
             \n\
                 func increment(c: Counter)\n\
                     requires c.count >= 0\n\
                     ensures c.count == old(c.count) + 1\n\
                 {{\n\
                     mutate state {{\n\
                         c.count += 1\n\
                     }}\n\
                 }}\n\
             \n\
                func decrement(c: Counter)\n\
                    requires c.count > 0\n\
                    ensures c.count == old(c.count) - 1\n\
                {{\n\
                    mutate state {{\n\
                        c.count -= 1\n\
                    }}\n\
                }}\n\
             }}\n",
            mod = module_name,
        ),
        "dual-backend" => format!(
            "// Dual-backend template: one Rust module (cpu_bound) and one Go module (network_io).\n\
             \n\
             @boundary(cpu_bound)\n\
             module {mod}Rust {{\n\
             \n\
                 invariant Counter {{\n\
                     count >= 0\n\
                 }}\n\
             \n\
                 func increment(c: Counter)\n\
                     requires c.count >= 0\n\
                     ensures c.count == old(c.count) + 1\n\
                 {{\n\
                     mutate state {{\n\
                         c.count += 1\n\
                     }}\n\
                 }}\n\
             }}\n\
             \n\
             @boundary(network_io)\n\
             module {mod}Go {{\n\
             \n\
                 invariant Box {{\n\
                     total >= 0\n\
                 }}\n\
             \n\
                 func handle(b: Box, req: Int)\n\
                     requires req >= 0\n\
                     ensures b.total == old(b.total) + req\n\
                 {{\n\
                     mutate state {{\n\
                         b.total += req\n\
                     }}\n\
                 }}\n\
             }}\n",
            mod = module_name,
        ),
        "eject" => format!(
            "@boundary(cpu_bound)\n\
             module {mod} {{\n\
             \n\
                 invariant Counter {{\n\
                     count >= 0\n\
                 }}\n\
             \n\
                 // @eject marks this function as a trusted opaque block.\n\
                 // The compiler generates a guard wrapper that enforces contracts at runtime.\n\
                 @eject\n\
                 func process(c: Counter, by: Int)\n\
                     requires c.count >= by\n\
                     ensures c.count == old(c.count) - by\n\
                 {{\n\
                     mutate state {{\n\
                         c.count -= by\n\
                     }}\n\
                 }}\n\
             }}\n",
            mod = module_name,
        ),
        "real-time" => format!(
            "// Real-time / zero-allocation template: routes to Rust (no GC) and is\n\
        // rejected by `telos project --check --strict-rt` if accidentally routed to Go.\n\
        @boundary(real_time, zero_allocation)\n\
        module {mod} {{\n\
        \n\
        invariant Controller {{\n\
        \n\
        setpoint >= 0\n\
        \n\
        }}\n\
        \n\
        func step(c: Controller, dt: Int)\n\
        requires dt > 0\n\
        ensures c.setpoint == old(c.setpoint) + dt\n\
        {{\n\
        mutate state {{\n\
        c.setpoint += dt\n\
        }}\n\
        }}\n\
        }}\n",
            mod = module_name,
        ),
        "python-ml" => format!(
            "// ML / JAX template: routed to the Python backend (@dataclass + runtime asserts).\n\
        @boundary(ml_training)\n\
        module {mod} {{\n\
        \n\
        invariant Tensor {{\n\
        \n\
        dims > 0\n\
        \n\
        }}\n\
        \n\
        func normalize(t: Tensor, scale: Int)\n\
        requires scale >= 0\n\
        ensures t.dims == old(t.dims) + scale\n\
        {{\n\
        mutate state {{\n\
        t.dims = t.dims + scale\n\
        }}\n\
        }}\n\
        }}\n",
            mod = module_name,
        ),
        "cross-module" => format!(
            "// Cross-module template: one module declares an invariant type, another\n\
        // references it (exercises the global type-resolution pass in tpt-telos-ir).\n\
        @boundary(cpu_bound)\n\
        module {mod}Core {{\n\
        \n\
        invariant Counter {{\n\
        \n\
        count >= 0\n\
        \n\
        }}\n\
        }}\n\
        \n\
        @boundary(cpu_bound)\n\
        module {mod}Ops {{\n\
        \n\
        func bump(c: Counter)\n\
        requires c.count >= 0\n\
        ensures c.count == old(c.count) + 1\n\
        {{\n\
        mutate state {{\n\
        c.count += 1\n\
        }}\n\
        }}\n\
        }}\n",
            mod = module_name,
        ),
        other => {
            return Err(format!(
                "unknown template `{other}`; valid options: simple, dual-backend, eject, \
                 real-time, python-ml, cross-module"
            ))
        }
    };
    Ok(content)
}

fn run_parse(file: &str, json: bool) -> Result<(), String> {
    let modules = load_modules(file)?;
    if json {
        let json_modules: Vec<JsonParseModule> = modules
            .iter()
            .map(|m| {
                let functions = m
                    .items
                    .iter()
                    .filter_map(|item| {
                        if let Item::Func(f) = item {
                            Some(JsonParseFunc {
                                name: f.name.clone(),
                                params: f
                                    .params
                                    .iter()
                                    .map(|p| format!("{}: {}", p.name, render_type(&p.ty)))
                                    .collect(),
                                requires: f.requires.iter().map(pretty_expr).collect(),
                                ensures: f.ensures.iter().map(pretty_expr).collect(),
                            })
                        } else {
                            None
                        }
                    })
                    .collect();
                let invariants = m
                    .items
                    .iter()
                    .filter_map(|item| {
                        if let Item::Invariant(i) = item {
                            Some(i.name.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                JsonParseModule {
                    name: m.name.clone(),
                    functions,
                    invariants,
                }
            })
            .collect();
        let output = JsonParseOutput {
            file: file.to_string(),
            modules: json_modules,
        };
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        for m in &modules {
            println!("{}", render_module(m));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Machine-readable JSON output types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct JsonLocation {
    line: usize,
    column: usize,
}

#[derive(Serialize)]
struct JsonCheck {
    description: String,
    passed: bool,
    is_ensures: bool,
    is_approximation: bool,
    counterexample: Option<std::collections::HashMap<String, i64>>,
    or_group: Option<usize>,
    location: Option<JsonLocation>,
}

#[derive(Serialize)]
struct JsonFuncResult {
    func_name: String,
    all_passed: bool,
    checks: Vec<JsonCheck>,
}

#[derive(Serialize)]
struct JsonVerifyOutput {
    file: String,
    passed: bool,
    functions: Vec<JsonFuncResult>,
}

#[derive(Serialize)]
struct JsonBuildOutput {
    file: String,
    passed: bool,
    out_dir: String,
    functions: Vec<JsonFuncResult>,
    proof_hash: Option<String>,
}

#[derive(Serialize)]
struct JsonProjectOutput {
    file: String,
    passed: bool,
    out_dir: String,
    functions: Vec<JsonFuncResult>,
    proof_hash: Option<String>,
    has_rust: bool,
    has_go: bool,
    has_python: bool,
    has_ffi: bool,
}

#[derive(Serialize)]
struct JsonParseFunc {
    name: String,
    params: Vec<String>,
    requires: Vec<String>,
    ensures: Vec<String>,
}

#[derive(Serialize)]
struct JsonParseModule {
    name: String,
    functions: Vec<JsonParseFunc>,
    invariants: Vec<String>,
}

#[derive(Serialize)]
struct JsonParseOutput {
    file: String,
    modules: Vec<JsonParseModule>,
}

#[derive(Serialize)]
struct JsonEjectEntry {
    module: String,
    func: String,
    lang: String,
}

#[derive(Serialize)]
struct JsonEjectOutput {
    file: String,
    func: Option<String>,
    ejected: Vec<JsonEjectEntry>,
    rust_code: Option<String>,
    go_code: Option<String>,
    files: Vec<String>,
}

fn collect_verify_output(
    problems: &[tpt_telos_ir::VerificationProblem],
) -> (Vec<JsonFuncResult>, bool) {
    let mut functions = Vec::new();
    let mut overall = true;
    for problem in problems {
        let result = verify(problem);
        let checks: Vec<JsonCheck> = result
            .checks
            .iter()
            .map(|c| JsonCheck {
                description: c.description.clone(),
                passed: c.passed,
                is_ensures: c.is_ensures,
                is_approximation: c.is_approximation,
                counterexample: c.counterexample.clone(),
                or_group: c.or_group,
                location: c
                    .location
                    .map(|(line, col)| JsonLocation { line, column: col }),
            })
            .collect();
        if !result.all_passed {
            overall = false;
        }
        functions.push(JsonFuncResult {
            func_name: result.func_name,
            all_passed: result.all_passed,
            checks,
        });
    }
    (functions, overall)
}

fn run_verify(file: &str, solver: &str, json: bool, watch: bool) -> Result<bool, String> {
    // Configure solver backend.
    match solver {
        "fourier-motzkin" => {}
        "z3" => {
            #[cfg(feature = "z3")]
            {
                if !tpt_telos_verifier::z3_solver::is_z3_available() {
                    eprintln!("warning: Z3 not found at runtime; falling back to Fourier-Motzkin");
                } else {
                    tpt_telos_verifier::set_solver_backend(tpt_telos_verifier::SolverBackend::Z3);
                }
            }
            #[cfg(not(feature = "z3"))]
            {
                eprintln!(
                    "warning: Z3 solver requires building with `--features z3`; \
                     falling back to Fourier-Motzkin"
                );
            }
        }
        other => return Err(format!("unknown solver backend: `{other}`")),
    }

    let color = use_color();

    let run_once = |json_mode: bool| -> Result<bool, String> {
        let modules = load_modules(file)?;
        let problems = tpt_telos_ir::extract(&modules)?;

        if problems.is_empty() {
            eprintln!("warning: no functions found to verify in `{file}`");
        }

        if json_mode {
            let (functions, overall) = collect_verify_output(&problems);
            let output = JsonVerifyOutput {
                file: file.to_string(),
                passed: overall,
                functions,
            };
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
            return Ok(overall);
        }

        let src = fs::read_to_string(file).unwrap_or_default();
        let lines: Vec<&str> = src.lines().collect();

        let mut overall = true;
        println!("Verifying {}\n", file);
        for problem in &problems {
            let result = verify(problem);
            println!("  function {}:", result.func_name);

            let independent: Vec<_> = result
                .checks
                .iter()
                .filter(|c| c.or_group.is_none())
                .collect();
            let mut groups: std::collections::HashMap<usize, Vec<&_>> =
                std::collections::HashMap::new();
            for c in &result.checks {
                if let Some(g) = c.or_group {
                    groups.entry(g).or_default().push(c);
                }
            }

            for check in &independent {
                let tag = if check.passed {
                    paint(color, GREEN, "PASS")
                } else {
                    paint(color, RED, "FAIL")
                };
                let kind = if check.is_ensures { "ensures " } else { "" };
                let approx = if check.is_approximation {
                    paint(color, YELLOW, " [interval-bounded]")
                } else {
                    String::new()
                };
                println!("    [{}] {}{}{}", tag, kind, check.description, approx);
                if !check.passed {
                    overall = false;
                    if let Some(ce) = &check.counterexample {
                        let bindings: Vec<_> =
                            ce.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
                        println!("      counterexample: {{{}}}", bindings.join(", "));
                    }
                    if let Some(loc) = check.location {
                        emit_caret(color, file, &lines, loc);
                    }
                }
            }

            let mut group_keys: Vec<_> = groups.keys().copied().collect();
            group_keys.sort();
            for gk in group_keys {
                let members = &groups[&gk];
                let group_any_passed = members.iter().any(|c| c.passed);
                let group_tag = if group_any_passed {
                    paint(color, GREEN, "PASS")
                } else {
                    paint(color, RED, "FAIL")
                };
                println!("    [{}] disjunction group {}:", group_tag, gk);
                for check in members {
                    let tag = if check.passed {
                        paint(color, GREEN, "PASS")
                    } else {
                        paint(color, RED, "FAIL")
                    };
                    let kind = if check.is_ensures { "ensures " } else { "" };
                    let approx = if check.is_approximation {
                        paint(color, YELLOW, " [interval-bounded]")
                    } else {
                        String::new()
                    };
                    println!("      [{}] {}{}{}", tag, kind, check.description, approx);
                    if !check.passed {
                        if let Some(ce) = &check.counterexample {
                            let bindings: Vec<_> =
                                ce.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
                            println!("        counterexample: {{{}}}", bindings.join(", "));
                        }
                        if let Some(loc) = check.location {
                            emit_caret(color, file, &lines, loc);
                        }
                    }
                }
                if !group_any_passed {
                    overall = false;
                }
            }

            let status = if result.all_passed {
                paint(color, GREEN, "PASS")
            } else {
                paint(color, RED, "FAIL")
            };
            println!("    => {}\n", status);
        }

        if overall {
            println!(
                "{}",
                paint(color, GREEN, "RESULT: all constraints satisfied.")
            );
        } else {
            println!(
                "{}",
                paint(color, RED, "RESULT: verification failed (see FAIL above).")
            );
            println!(
                "\n{}",
                paint(
                    color,
                    YELLOW,
                    "hint: some failures may be solver limitations (FM solver is incomplete \
                     for nonlinear arithmetic); try --solver z3 for exact results"
                )
            );
        }
        Ok(overall)
    };

    if !watch {
        return run_once(json);
    }

    // Watch mode: re-verify on any `.telos` change in the file's directory,
    // after a short debounce window so multi-file saves re-run once.
    watch_loop(file, || run_once(false))
}

fn make_agent(llm: bool) -> Result<Box<dyn tpt_telos_agent::CodeAgent>, String> {
    if llm {
        #[cfg(feature = "llm")]
        {
            Ok(Box::new(LlmAgent::from_env()?))
        }
        #[cfg(not(feature = "llm"))]
        {
            Err("the `llm` agent requires building telos with the `llm` feature".to_string())
        }
    } else {
        Ok(Box::new(StaticAgent::new()))
    }
}

fn run_transpile(file: &str, llm: bool, out: Option<String>) -> Result<(), String> {
    let modules = load_modules(file)?;
    let agent = make_agent(llm)?;

    println!("Agentic transpiler (agent: {})\n", agent.name());
    let mut outcomes = Vec::new();
    for m in &modules {
        let funcs = transpile_module(m, agent.as_ref())?;
        for o in &funcs {
            println!(
                "  {} :: {}  [target: {}]",
                m.name,
                o.func_name,
                o.target.as_str()
            );
            for step in &o.iterations {
                let ce = step
                    .counterexample
                    .as_ref()
                    .map(|m| {
                        let mut e: Vec<_> = m.iter().collect();
                        e.sort_by(|a, b| a.0.cmp(b.0));
                        e.iter()
                            .map(|(k, v)| format!("{}={}", k, v))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                println!(
                    "    iter {} [{}] verified={} {}",
                    step.iteration,
                    step.action,
                    step.passed,
                    if ce.is_empty() {
                        String::new()
                    } else {
                        format!("counterexample: {{{}}}", ce)
                    }
                );
            }
            println!(
                "    => {} (agent: {})\n",
                if o.verified { "VERIFIED" } else { "UNVERIFIED" },
                o.agent
            );
        }
        outcomes.extend(funcs);
    }

    let rust = generate_program(&modules, &outcomes);
    match out {
        Some(path) => {
            fs::write(&path, rust).map_err(|e| format!("cannot write `{path}`: {e}"))?;
            println!("Wrote generated Rust to {path}");
        }
        None => {
            println!("----- generated Rust -----\n");
            println!("{rust}");
        }
    }

    Ok(())
}

fn run_build(
    file: &str,
    out_dir: &str,
    llm: bool,
    solver: &str,
    json: bool,
    watch: bool,
) -> Result<bool, String> {
    let run_once = || -> Result<bool, String> {
        // Configure solver backend (same logic as run_verify).
        match solver {
            "fourier-motzkin" => {}
            "z3" => {
                #[cfg(feature = "z3")]
                {
                    if !tpt_telos_verifier::z3_solver::is_z3_available() {
                        eprintln!(
                            "warning: Z3 not found at runtime; falling back to Fourier-Motzkin"
                        );
                    } else {
                        tpt_telos_verifier::set_solver_backend(
                            tpt_telos_verifier::SolverBackend::Z3,
                        );
                    }
                }
                #[cfg(not(feature = "z3"))]
                {
                    eprintln!(
                        "warning: Z3 solver requires building with `--features z3`; \
                     falling back to Fourier-Motzkin"
                    );
                }
            }
            other => return Err(format!("unknown solver backend: `{other}`")),
        }

        let src_bytes = fs::read(file).map_err(|e| format!("cannot read `{file}`: {e}"))?;
        let modules = load_modules(file)?;
        let agent = make_agent(llm)?;

        let mut outcomes = Vec::new();
        let mut all_verified = true;
        for m in &modules {
            for o in transpile_module(m, agent.as_ref())? {
                if !o.verified {
                    all_verified = false;
                }
                outcomes.push(o);
            }
        }

        // Generate proof manifest before appending the static to the Rust source.
        let manifest = proof::generate_manifest(&src_bytes, &outcomes);
        let proof_static = proof::render_rust_proof_static(&manifest);

        let mut rust = generate_program(&modules, &outcomes);
        rust.push_str(&proof_static);

        let crate_dir = std::path::Path::new(out_dir);
        fs::create_dir_all(crate_dir.join("src"))
            .map_err(|e| format!("cannot create {out_dir}: {e}"))?;
        fs::write(crate_dir.join("src/lib.rs"), &rust)
            .map_err(|e| format!("cannot write lib.rs: {e}"))?;
        fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"generated\"\nversion = \"0.1.0\"\n\
         edition = \"2021\"\n\n[dependencies]\n\n[workspace]\n",
        )
        .map_err(|e| format!("cannot write Cargo.toml: {e}"))?;

        // Write proof manifest alongside generated code.
        let proof_json = proof::to_json(&manifest);
        fs::write(crate_dir.join("telos-proof.json"), &proof_json)
            .map_err(|e| format!("cannot write telos-proof.json: {e}"))?;

        // Compile the generated crate.
        let status = std::process::Command::new("cargo")
            .arg("build")
            .arg("--manifest-path")
            .arg(crate_dir.join("Cargo.toml"))
            .status();

        let compile_ok = match &status {
            Ok(s) => s.success(),
            Err(_) => false,
        };

        if json {
            let problems = tpt_telos_ir::extract(&modules)?;
            let (functions, _) = collect_verify_output(&problems);
            let output = JsonBuildOutput {
                file: file.to_string(),
                passed: all_verified && compile_ok,
                out_dir: out_dir.to_string(),
                functions,
                proof_hash: Some(manifest.manifest_hash.clone()),
            };
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
            if !compile_ok {
                return Err("generated Rust failed to compile".into());
            }
            return Ok(all_verified);
        }

        println!("Generated crate written to {out_dir}/");
        println!("Proof manifest written → {out_dir}/telos-proof.json");
        if !all_verified {
            println!("WARNING: some functions were not mathematically verified.");
        }

        println!("Compiling generated Rust with cargo...\n");
        match status {
            Ok(s) if s.success() => {
                println!("\nBUILD: generated Rust compiles successfully.");
                Ok(all_verified)
            }
            Ok(s) => {
                eprintln!("\nBUILD: cargo exited with {s}");
                Err("generated Rust failed to compile".into())
            }
            Err(e) => Err(format!(
                "could not invoke cargo (is it on PATH?): {e}. \
             The generated Rust was still written to {out_dir}/.",
            )),
        }
    };
    if !watch {
        run_once()
    } else {
        watch_loop(file, run_once)
    }
}

fn run_project(
    file: &str,
    out_dir: &str,
    llm: bool,
    check: bool,
    strict_rt: bool,
    json: bool,
    watch: bool,
) -> Result<bool, String> {
    let run_once = || -> Result<bool, String> {
        let src_bytes = fs::read(file).map_err(|e| format!("cannot read `{file}`: {e}"))?;
        let modules = load_modules(file)?;
        let agent = make_agent(llm)?;

        let mut outcomes = Vec::new();
        let mut all_verified = true;
        for m in &modules {
            let _target = tpt_telos_router::route(&m.attributes).target;
            for o in transpile_module(m, agent.as_ref())? {
                if !o.verified {
                    all_verified = false;
                }
                outcomes.push(o);
            }
        }

        let project = generate_project(&modules, &outcomes)?;

        // --strict-rt: promote routing conflicts to hard errors.
        if strict_rt {
            let has_conflict = project.diagnostics.iter().any(|d| {
                d.kind == tpt_telos_router::DiagnosticKind::RealTimeGoConflict
                    || d.kind == tpt_telos_router::DiagnosticKind::ZeroAllocGoConflict
            });
            if has_conflict {
                return Err(
                    "strict-rt: routing conflict detected (real_time/zero_allocation \
                 module routed to Go; see warnings above)"
                        .into(),
                );
            }
        }

        let root = std::path::Path::new(out_dir);
        project
            .write(root)
            .map_err(|e| format!("cannot write project to {out_dir}: {e}"))?;
        if project.has_go {
            canonicalize_go(&root.join("go"));
        }

        // Write proof manifest.
        let manifest = proof::generate_manifest(&src_bytes, &outcomes);
        let proof_json = proof::to_json(&manifest);
        fs::write(root.join("telos-proof.json"), &proof_json)
            .map_err(|e| format!("cannot write telos-proof.json: {e}"))?;

        if json {
            let mut ok = all_verified;

            if check {
                if project.has_rust {
                    let status = std::process::Command::new("cargo")
                        .arg("build")
                        .arg("--manifest-path")
                        .arg(root.join("rust/Cargo.toml"))
                        .status();
                    if let Ok(s) = status {
                        if !s.success() {
                            ok = false;
                        }
                    } else {
                        ok = false;
                    }
                }
                if project.has_go {
                    let status = std::process::Command::new("go")
                        .arg("build")
                        .arg("./...")
                        .current_dir(root.join("go"))
                        .status();
                    if let Ok(s) = status {
                        if !s.success() {
                            ok = false;
                        }
                    } else {
                        ok = false;
                    }
                }
            }

            let problems = tpt_telos_ir::extract(&modules)?;
            let (functions, _) = collect_verify_output(&problems);
            let output = JsonProjectOutput {
                file: file.to_string(),
                passed: ok,
                out_dir: out_dir.to_string(),
                functions,
                proof_hash: Some(manifest.manifest_hash.clone()),
                has_rust: project.has_rust,
                has_go: project.has_go,
                has_python: project.has_python,
                has_ffi: project.has_ffi,
            };
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
            return Ok(ok);
        }

        println!("\nProject written to {out_dir}/");
        for f in &project.files {
            println!("  {}", f.path);
        }
        println!("  telos-proof.json");
        println!(
            "\nBackends: rust={} go={} python={} ffi_bridge={}",
            project.has_rust, project.has_go, project.has_python, project.has_ffi
        );
        println!("Proof manifest written → {out_dir}/telos-proof.json");
        if !all_verified {
            println!("WARNING: some functions were not mathematically verified.");
        }

        if !check {
            return Ok(all_verified);
        }

        let mut ok = all_verified;

        // Compile the Rust crate.
        if project.has_rust {
            println!("\nCompiling Rust backend with cargo...");
            let status = std::process::Command::new("cargo")
                .arg("build")
                .arg("--manifest-path")
                .arg(root.join("rust/Cargo.toml"))
                .status()
                .map_err(|e| format!("could not invoke cargo: {e}"))?;
            if status.success() {
                println!("  Rust backend compiles.");
            } else {
                eprintln!("  Rust backend failed to compile ({status}).");
                ok = false;
            }
        }

        // Vet the Go package.
        if project.has_go {
            println!("\nBuilding Go backend with go...");
            let status = std::process::Command::new("go")
                .arg("build")
                .arg("./...")
                .current_dir(root.join("go"))
                .status()
                .map_err(|e| format!("could not invoke go (is it on PATH?): {e}"))?;
            if status.success() {
                println!("  Go backend compiles.");
            } else {
                eprintln!("  Go backend failed to compile ({status}).");
                ok = false;
            }

            let out = std::process::Command::new("gofmt")
                .arg("-l")
                .arg(".")
                .current_dir(root.join("go"))
                .output();
            match out {
                Ok(o) if o.status.success() => {
                    let listed = String::from_utf8_lossy(&o.stdout);
                    if listed.trim().is_empty() {
                        println!("  Go sources (incl. cgo FFI) are well-formed.");
                    } else {
                        eprintln!("  Go sources not gofmt-clean:\n{}", listed);
                        ok = false;
                    }
                }
                Ok(o) => {
                    eprintln!(
                        "  gofmt reported errors: {}",
                        String::from_utf8_lossy(&o.stderr)
                    );
                    ok = false;
                }
                Err(e) => {
                    eprintln!("  could not invoke gofmt: {e}");
                }
            }
        }

        if ok {
            println!("\nPROJECT: dual-backend microservice builds successfully.");
        } else {
            println!("\nPROJECT: one or more backends failed.");
        }
        Ok(ok)
    };
    if !watch {
        run_once()
    } else {
        watch_loop(file, run_once)
    }
}

fn run_verify_manifest(manifest_path: &str, source_path: &str) -> Result<(), String> {
    let manifest_json = fs::read_to_string(manifest_path)
        .map_err(|e| format!("cannot read `{manifest_path}`: {e}"))?;
    let source_bytes =
        fs::read(source_path).map_err(|e| format!("cannot read `{source_path}`: {e}"))?;

    let result = proof::verify_manifest(&manifest_json, &source_bytes);

    if result.tampered {
        eprintln!("MANIFEST TAMPERED");
        if !result.source_hash_valid {
            eprintln!(
                "  source hash MISMATCH: expected {}, got {}",
                result.source_hash_expected, result.source_hash_actual
            );
        }
        if !result.manifest_hash_valid {
            eprintln!(
                "  manifest hash MISMATCH: expected {}, got {}",
                result.manifest_hash_expected, result.manifest_hash_actual
            );
        }
        return Err("manifest verification failed".into());
    }

    println!("MANIFEST OK");
    println!("  source hash:     {}", result.source_hash_expected);
    println!("  manifest hash:   {}", result.manifest_hash_expected);
    println!("  source unchanged: yes");
    println!("  manifest intact:  yes");
    Ok(())
}

fn run_eject(
    file: &str,
    out_dir: &str,
    only: Option<&str>,
    llm: bool,
    strict_rt: bool,
    json: bool,
) -> Result<(), String> {
    let mut modules = load_modules(file)?;

    // Bug 3 fix: when --func <name> is specified, verify the function exists
    // before we do any work. Without this check a non-existent name silently
    // falls through and any @eject-marked function is ejected instead.
    if let Some(func_name) = only {
        let found = modules.iter().any(|m| {
            m.items.iter().any(|item| {
                if let Item::Func(f) = item {
                    f.name == func_name
                } else {
                    false
                }
            })
        });
        if !found {
            return Err(format!(
                "error: function '{}' not found in {}",
                func_name, file
            ));
        }
    }

    let agent = make_agent(llm)?;

    // Transpile first so ejected opaque blocks are seeded with verified bodies.
    let mut outcomes = Vec::new();
    for m in &modules {
        outcomes.extend(transpile_module(m, agent.as_ref())?);
    }

    // Mark the requested functions as ejected. In-source `@eject` attributes are
    // always honored; here we additionally eject on demand from the CLI.
    let mut ejected: Vec<(String, String, String)> = Vec::new(); // (module, func, lang)
    for m in &mut modules {
        let lang = tpt_telos_router::route(&m.attributes)
            .target
            .as_str()
            .to_string();
        for item in &mut m.items {
            if let Item::Func(f) = item {
                let selected = only.map(|n| n == f.name).unwrap_or(true);
                if selected || f.is_ejected() {
                    if !f.is_ejected() {
                        f.attributes.push(Attribute {
                            name: "eject".to_string(),
                            args: vec![Arg::Flag(lang.clone())],
                        });
                    }
                    ejected.push((m.name.clone(), f.name.clone(), lang.clone()));
                }
            }
        }
    }

    if ejected.is_empty() {
        return Err(format!(
            "no matching function to eject{}",
            only.map(|n| format!(" (looked for `{n}`)"))
                .unwrap_or_default()
        ));
    }

    let project = generate_project(&modules, &outcomes)?;
    let root = std::path::Path::new(out_dir);

    // Surface routing diagnostics as warnings.
    for diag in &project.diagnostics {
        eprintln!(
            "WARNING [{}] {}",
            format!("{:?}", diag.kind).to_lowercase(),
            diag.message
        );
    }

    // --strict-rt: promote routing conflicts to hard errors.
    if strict_rt {
        let has_conflict = project.diagnostics.iter().any(|d| {
            d.kind == tpt_telos_router::DiagnosticKind::RealTimeGoConflict
                || d.kind == tpt_telos_router::DiagnosticKind::ZeroAllocGoConflict
        });
        if has_conflict {
            return Err(
                "strict-rt: routing conflict detected (real_time/zero_allocation \
                 module routed to Go; see warnings above)"
                    .into(),
            );
        }
    }

    project
        .write(root)
        .map_err(|e| format!("cannot write ejected project to {out_dir}: {e}"))?;
    if project.has_go {
        canonicalize_go(&root.join("go"));
    }

    // Write a manifest recording what was ejected (trusted opaque blocks).
    let mut manifest = String::from("{\n  \"ejected\": [\n");
    for (i, (module, func, lang)) in ejected.iter().enumerate() {
        let comma = if i + 1 < ejected.len() { "," } else { "" };
        manifest.push_str(&format!(
            "    {{ \"module\": \"{}\", \"func\": \"{}\", \"lang\": \"{}\" }}{}\n",
            module, func, lang, comma
        ));
    }
    manifest.push_str("  ]\n}\n");
    std::fs::write(root.join("telos-eject.json"), manifest)
        .map_err(|e| format!("cannot write manifest: {e}"))?;

    if json {
        let rust_code = project
            .files
            .iter()
            .find(|f| f.path == "rust/src/lib.rs")
            .map(|f| f.contents.clone());
        let go_code = project
            .files
            .iter()
            .find(|f| f.path == "go/service.go")
            .map(|f| f.contents.clone());
        let files: Vec<String> = project
            .files
            .iter()
            .map(|f| f.path.clone())
            .chain(std::iter::once("telos-eject.json".to_string()))
            .collect();
        let output = JsonEjectOutput {
            file: file.to_string(),
            func: only.map(|s| s.to_string()),
            ejected: ejected
                .iter()
                .map(|(module, func, lang)| JsonEjectEntry {
                    module: module.clone(),
                    func: func.clone(),
                    lang: lang.clone(),
                })
                .collect(),
            rust_code,
            go_code,
            files,
        };
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
        return Ok(());
    }

    println!("Ejected {} function(s) to {}/", ejected.len(), out_dir);
    for (module, func, lang) in &ejected {
        println!(
            "  {}::{} -> raw {} (opaque block + contract guard)",
            module, func, lang
        );
    }
    for f in &project.files {
        println!("  {}", f.path);
    }
    println!("  telos-eject.json");
    println!(
        "\nEjected code is a trusted opaque block; the generated guard enforces the\n\
         original requires/ensures contracts at the boundary."
    );
    Ok(())
}

// ---- lightweight AST rendering ----

fn render_module(m: &Module) -> String {
    let attrs: Vec<String> = m
        .attributes
        .iter()
        .map(|a| {
            if a.args.is_empty() {
                format!("@{}", a.name)
            } else {
                let args = a
                    .args
                    .iter()
                    .map(|arg| match arg {
                        Arg::Flag(f) => f.clone(),
                        Arg::Kv(k, v) => format!("{}={}", k, render_literal(v)),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("@{}(", a.name) + &args + ")"
            }
        })
        .collect();
    let header = if attrs.is_empty() {
        format!("module {}", m.name)
    } else {
        format!("{} module {}", attrs.join(" "), m.name)
    };
    let items = m
        .items
        .iter()
        .map(render_item)
        .collect::<Vec<_>>()
        .join("\n");
    format!("{}{{\n{}\n}}", header, indent(&items))
}

fn render_item(item: &Item) -> String {
    match item {
        Item::Invariant(i) => render_invariant(i),
        Item::Func(f) => render_func(f),
        Item::Struct(s) => {
            let fields: Vec<_> = s
                .fields
                .iter()
                .map(|f| format!("{}: {}", f.name, render_type(&f.ty)))
                .collect();
            format!("struct {} {{ {} }}", s.name, fields.join(", "))
        }
        Item::Enum(e) => {
            let variants: Vec<_> = e
                .variants
                .iter()
                .map(|v| {
                    if v.fields.is_empty() {
                        v.name.clone()
                    } else {
                        let fields: Vec<_> = v
                            .fields
                            .iter()
                            .map(|f| format!("{}: {}", f.name, render_type(&f.ty)))
                            .collect();
                        format!("{} {{ {} }}", v.name, fields.join(", "))
                    }
                })
                .collect();
            format!("enum {} {{ {} }}", e.name, variants.join(", "))
        }
    }
}

fn render_invariant(i: &Invariant) -> String {
    let body = i
        .constraints
        .iter()
        .map(pretty_expr)
        .collect::<Vec<_>>()
        .join("; ");
    format!("invariant {} {{ {} }}", i.name, body)
}

fn render_func(f: &Func) -> String {
    let params = f
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, render_type(&p.ty)))
        .collect::<Vec<_>>()
        .join(", ");
    let clauses: Vec<String> = f
        .requires
        .iter()
        .map(|e| format!("requires {}", pretty_expr(e)))
        .chain(
            f.ensures
                .iter()
                .map(|e| format!("ensures {}", pretty_expr(e))),
        )
        .collect();
    let body = f
        .body
        .iter()
        .map(render_stmt)
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = format!("func {}({})", f.name, params);
    if !clauses.is_empty() {
        out += "\n";
        out += &indent(&clauses.join("\n"));
    }
    if f.elided {
        out += ";";
    } else {
        out += "\n{\n";
        out += &indent(&body);
        out += "\n}";
    }
    out
}

fn render_stmt(s: &Stmt) -> String {
    match s {
        Stmt::MutateState(assigns) => {
            let inner = assigns
                .iter()
                .map(render_assign)
                .collect::<Vec<_>>()
                .join("\n");
            format!("mutate state {{\n{}\n}}", indent(&inner))
        }
        Stmt::Assign(a) => render_assign(a),
        Stmt::Let(lb) => {
            let ty = lb
                .ty
                .as_ref()
                .map(|t| format!(": {}", render_type(t)))
                .unwrap_or_default();
            format!("let {}{} = {};", lb.name, ty, pretty_expr(&lb.value))
        }
        Stmt::If(is) => {
            let mut out = format!("if {} {{\n", pretty_expr(&is.condition));
            let then: Vec<_> = is.then_body.iter().map(render_stmt).collect();
            out += &indent(&then.join("\n"));
            out += "\n}";
            if let Some(else_body) = &is.else_body {
                let els: Vec<_> = else_body.iter().map(render_stmt).collect();
                out += " else {\n";
                out += &indent(&els.join("\n"));
                out += "\n}";
            }
            out
        }
        Stmt::Match(ms) => {
            let arms: Vec<_> = ms
                .arms
                .iter()
                .map(|a| {
                    let body: Vec<_> = a.body.iter().map(render_stmt).collect();
                    format!(
                        "{} => {{\n{}\n}}",
                        render_pattern(&a.pattern),
                        indent(&body.join("\n"))
                    )
                })
                .collect();
            format!(
                "match {} {{\n{}\n}}",
                pretty_expr(&ms.scrutinee),
                indent(&arms.join("\n"))
            )
        }
        Stmt::Return(e) => match e {
            Some(expr) => format!("return {};", pretty_expr(expr)),
            None => "return;".to_string(),
        },
    }
}

fn render_assign(a: &Assign) -> String {
    let op = match a.op {
        AssignOp::Set => "=",
        AssignOp::Add => "+=",
        AssignOp::Sub => "-=",
    };
    format!(
        "{} {} {};",
        pretty_expr(&a.target),
        op,
        pretty_expr(&a.value)
    )
}

fn render_type(t: &Type) -> String {
    match t {
        Type::Named(s) => match s.as_str() {
            "Float32" => "f32".to_string(),
            "Float64" => "f64".to_string(),
            "Int" => "i64".to_string(),
            "PositiveInt" => "i64".to_string(),
            other => other.to_string(),
        },
        Type::Generic(name, args) => {
            let args: Vec<_> = args.iter().map(render_type).collect();
            format!("{}<{}>", name, args.join(", "))
        }
        Type::Tuple(elems) => {
            let elems: Vec<_> = elems.iter().map(render_type).collect();
            format!("({})", elems.join(", "))
        }
        Type::Array(elem, len) => format!("[{}; {}]", render_type(elem), len),
        Type::Slice(elem) => format!("[{}]", render_type(elem)),
    }
}

fn render_literal(l: &Literal) -> String {
    match l {
        Literal::Int(n) => n.to_string(),
        Literal::Ident(s) => s.clone(),
    }
}

fn pretty_expr(e: &Expr) -> String {
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Var(v) => v.clone(),
        Expr::Field { base, field } => format!("{}.{}", base, field),
        Expr::Old(inner) => format!("old({})", pretty_expr(inner)),
        Expr::Unary { op, expr } => match op {
            UnOp::Neg => format!("-{}", pretty_expr(expr)),
        },
        Expr::Bin { op, lhs, rhs } => {
            let s = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Eq => "==",
                BinOp::Ne => "!=",
                BinOp::Lt => "<",
                BinOp::Le => "<=",
                BinOp::Gt => ">",
                BinOp::Ge => ">=",
                BinOp::And => "&&",
                BinOp::Or => "||",
            };
            format!("{} {} {}", pretty_expr(lhs), s, pretty_expr(rhs))
        }
        Expr::Call(c) => {
            let args: Vec<_> = c.args.iter().map(pretty_expr).collect();
            format!("{}({})", c.func, args.join(", "))
        }
        Expr::MethodCall(m) => {
            let args: Vec<_> = m.args.iter().map(pretty_expr).collect();
            format!(
                "{}.{}({})",
                pretty_expr(&m.receiver),
                m.method,
                args.join(", ")
            )
        }
        Expr::Index(i) => format!("{}[{}]", pretty_expr(&i.receiver), pretty_expr(&i.index)),
        Expr::If(i) => format!(
            "if {} {{ {} }} else {{ {} }}",
            pretty_expr(&i.condition),
            pretty_expr(&i.then_expr),
            pretty_expr(&i.else_expr)
        ),
        Expr::Match(m) => {
            let arms: Vec<_> = m
                .arms
                .iter()
                .map(|a| format!("... => {}", pretty_expr(&a.expr)))
                .collect();
            format!(
                "match {} {{ {} }}",
                pretty_expr(&m.scrutinee),
                arms.join(", ")
            )
        }
        Expr::Try(e) => format!("{}?", pretty_expr(e)),
        Expr::Forall(f) => format!(
            "forall {}: {} {{ {} }}",
            f.var,
            f.var_ty.name(),
            pretty_expr(&f.body)
        ),
        Expr::Aggregate(a) => {
            let args: Vec<_> = a.args.iter().map(pretty_expr).collect();
            format!("{}({})", a.op.op_name(), args.join(", "))
        }
        Expr::Range { lo, hi } => format!("{}..{}", pretty_expr(lo), pretty_expr(hi)),
    }
}

fn render_pattern(p: &Pattern) -> String {
    match p {
        Pattern::Literal(n) => n.to_string(),
        Pattern::Var(v) => v.clone(),
        Pattern::Constructor(name, fields) => {
            if fields.is_empty() {
                name.clone()
            } else {
                let fields: Vec<_> = fields.iter().map(render_pattern).collect();
                format!("{}({})", name, fields.join(", "))
            }
        }
        Pattern::Wildcard => "_".to_string(),
    }
}

fn indent(s: &str) -> String {
    s.lines()
        .map(|line| {
            if line.is_empty() {
                line.to_string()
            } else {
                format!("    {}", line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
