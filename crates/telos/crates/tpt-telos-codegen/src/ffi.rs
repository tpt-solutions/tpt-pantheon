//! Automatic FFI bridge generation for tpt-telos (Phase 3).
//!
//! The context router splits a program into Rust-target and Go-target modules.
//! This module generates the glue that lets the two backends call each other
//! *without any hand-written boilerplate*, using a stable C ABI over `int64`
//! cells:
//!
//!   * every struct parameter is flattened into one C cell per field
//!     (`base_field`); mutated fields are passed by pointer (`int64_t*`),
//!     read-only fields by value (`int64_t`),
//!   * a scalar parameter is a single `int64_t`,
//!   * a scalar return value is the C function's `int64_t` result.
//!
//! Three artifacts are produced:
//!   1. `telos_ffi.h` -- C prototypes for the Rust exports (what Go calls).
//!   2. `ffi.rs` -- `#[no_mangle]` exports for Rust functions **and** `extern
//!      "C"` imports + safe wrappers for Go functions.
//!   3. `ffi.go` -- cgo wrappers that call the Rust exports **and** `//export`
//!      shims exposing Go functions to Rust.

use std::collections::HashMap;

use tpt_telos_parser::ast::*;
use tpt_telos_router::Target;

use crate::{analyze_func, collect_types, go::exported, InputParam, TypeFields};

// ---------------------------------------------------------------- type checks

/// True if `name` is an integer scalar type supported at FFI boundaries.
fn is_ffi_integer_type_name(name: &str) -> bool {
    matches!(name, "Int" | "PositiveInt")
}

/// Human-readable representation of a type (used in error messages).
fn type_display(ty: &Type) -> String {
    match ty {
        Type::Named(n) => n.clone(),
        Type::Generic(n, args) => {
            let args: Vec<_> = args.iter().map(type_display).collect();
            format!("{}<{}>", n, args.join(", "))
        }
        Type::Tuple(elems) => {
            let elems: Vec<_> = elems.iter().map(type_display).collect();
            format!("({})", elems.join(", "))
        }
        Type::Array(elem, n) => format!("[{}; {}]", type_display(elem), n),
        Type::Slice(elem) => format!("[{}]", type_display(elem)),
    }
}

/// Validate that every parameter of `f` can cross an FFI boundary.
///
/// Only integer scalar types (`Int`, `PositiveInt`) and structs whose every
/// field is one of those integer types are supported.  Everything else —
/// floats, booleans, strings, arrays, slices, generics, tuples — is rejected
/// with a descriptive error.
fn validate_ffi_params(f: &Func, types: &TypeFields, module_name: &str) -> Result<(), String> {
    for param in &f.params {
        match &param.ty {
            Type::Named(type_name) => {
                if is_ffi_integer_type_name(type_name) {
                    continue; // integer scalar — OK
                }
                match types.get(type_name.as_str()) {
                    Some(fields) if !fields.is_empty() => {
                        // Struct type: every field must be an integer type.
                        for (field_name, field_ty) in fields {
                            let ok = matches!(
                                field_ty,
                                Type::Named(n) if is_ffi_integer_type_name(n)
                            );
                            if !ok {
                                return Err(format!(
                                    "FFI boundary: unsupported type {} for field '{}' \
                                     in '{}::{}' — only integer types are supported at \
                                     FFI boundaries",
                                    type_display(field_ty),
                                    field_name,
                                    module_name,
                                    f.name
                                ));
                            }
                        }
                    }
                    _ => {
                        // Non-struct named type that is not a supported integer.
                        return Err(format!(
                            "FFI boundary: unsupported type {} for parameter '{}' \
                             in '{}::{}' — only integer types are supported at \
                             FFI boundaries",
                            type_name, param.name, module_name, f.name
                        ));
                    }
                }
            }
            other => {
                return Err(format!(
                    "FFI boundary: unsupported type {} for parameter '{}' \
                     in '{}::{}' — only integer types are supported at \
                     FFI boundaries",
                    type_display(other),
                    param.name,
                    module_name,
                    f.name
                ));
            }
        }
    }
    Ok(())
}

/// A single function lifted to the FFI boundary.
struct FfiFunc {
    module: String,
    func: String,
    target: Target,
    inputs: Vec<InputParam>,
    returns_scalar: bool,
}

impl FfiFunc {
    fn c_name(&self) -> String {
        format!("telos_{}_{}", self.module, self.func)
    }

    /// Flattened C parameters, in call order.
    fn c_params(&self) -> Vec<CParam> {
        let mut v = Vec::new();
        for input in &self.inputs {
            match input {
                InputParam::Scalar { name } => v.push(CParam {
                    name: name.clone(),
                    pointer: false,
                }),
                InputParam::Struct {
                    name,
                    mutated,
                    fields,
                    ..
                } => {
                    for f in fields {
                        v.push(CParam {
                            name: format!("{}_{}", name, f),
                            pointer: *mutated,
                        });
                    }
                }
            }
        }
        v
    }
}

#[derive(Clone)]
struct CParam {
    name: String,
    pointer: bool,
}

impl CParam {
    fn c_ty(&self) -> &'static str {
        if self.pointer {
            "int64_t*"
        } else {
            "int64_t"
        }
    }
}

/// The generated FFI artifacts.
pub struct FfiBridge {
    pub header: String,
    pub rust: String,
    pub go: String,
    pub go_package: String,
}

/// Build the FFI plan from every function in the program.
///
/// Returns `Err` if any parameter has a type that cannot be represented as an
/// `int64_t` cell across the C ABI (e.g. `Float64`, `Bool`, `String`, arrays).
fn plan(modules: &[Module], bodies: &HashMap<String, Vec<Stmt>>) -> Result<Vec<FfiFunc>, String> {
    let mut funcs = Vec::new();
    for m in modules {
        let mut types: TypeFields = HashMap::new();
        collect_types(m, &mut types);
        let target = tpt_telos_router::route(&m.attributes).target;
        for item in &m.items {
            if let Item::Func(f) = item {
                validate_ffi_params(f, &types, &m.name)?;
                let stmts = bodies
                    .get(&f.name)
                    .cloned()
                    .unwrap_or_else(|| f.body.clone());
                let analysis = analyze_func(f, &stmts, &types);
                funcs.push(FfiFunc {
                    module: m.name.clone(),
                    func: f.name.clone(),
                    target,
                    inputs: analysis.inputs,
                    returns_scalar: analysis.scalar_out.is_some(),
                });
            }
        }
    }
    Ok(funcs)
}

/// Generate the complete FFI bridge for a program.
///
/// Returns `Err` if any parameter type at an FFI-routed boundary is not an
/// integer type (`Int` / `PositiveInt`) or a struct whose fields are all
/// integer types.
pub fn generate_bridge(
    modules: &[Module],
    bodies: &HashMap<String, Vec<Stmt>>,
    go_package: &str,
) -> Result<FfiBridge, String> {
    let funcs = plan(modules, bodies)?;
    Ok(FfiBridge {
        header: gen_header(&funcs),
        rust: gen_rust(&funcs),
        go: gen_go(&funcs, go_package),
        go_package: go_package.to_string(),
    })
}

// ------------------------------------------------------------------ C header

fn gen_header(funcs: &[FfiFunc]) -> String {
    let mut out = String::new();
    out.push_str("/* Generated by tpt-telos. Automatic FFI bridge (C ABI). */\n");
    out.push_str("#ifndef TELOS_FFI_H\n#define TELOS_FFI_H\n\n#include <stdint.h>\n\n");
    out.push_str("/* Rust-generated functions, callable from Go. */\n");
    for f in funcs.iter().filter(|f| f.target == Target::Rust) {
        let ret = if f.returns_scalar { "int64_t" } else { "void" };
        let params = f
            .c_params()
            .iter()
            .map(|p| format!("{} {}", p.c_ty(), p.name))
            .collect::<Vec<_>>();
        let params = if params.is_empty() {
            "void".to_string()
        } else {
            params.join(", ")
        };
        out.push_str(&format!("{} {}({});\n", ret, f.c_name(), params));
    }
    out.push_str("\n#endif /* TELOS_FFI_H */\n");
    out
}

// ------------------------------------------------------------------ Rust side

fn gen_rust(funcs: &[FfiFunc]) -> String {
    let mut out = String::new();
    out.push_str("//! Generated by tpt-telos. Automatic FFI bridge (Rust side).\n");
    out.push_str("//! Rust exports use the C ABI so Go can call them; Go functions are\n");
    out.push_str("//! imported and wrapped in safe helpers so Rust can call them.\n");
    out.push_str("#![allow(clippy::missing_safety_doc, unused_unsafe)]\n\n");
    out.push_str("use crate::*;\n\n");

    // Rust exports (callable from Go).
    let rust_funcs: Vec<&FfiFunc> = funcs.iter().filter(|f| f.target == Target::Rust).collect();
    if !rust_funcs.is_empty() {
        out.push_str("// ===== Rust exports (callable from Go) =====\n\n");
        for f in &rust_funcs {
            out.push_str(&gen_rust_export(f));
            out.push('\n');
        }
    }

    // Go imports (callable from Rust).
    let go_funcs: Vec<&FfiFunc> = funcs.iter().filter(|f| f.target == Target::Go).collect();
    if !go_funcs.is_empty() {
        out.push_str("// ===== Go imports (callable from Rust) =====\n\n");
        out.push_str("extern \"C\" {\n");
        for f in &go_funcs {
            let params = f
                .c_params()
                .iter()
                .map(|p| format!("{}: {}", p.name, rust_c_ty(p)))
                .collect::<Vec<_>>()
                .join(", ");
            let ret = if f.returns_scalar { " -> i64" } else { "" };
            out.push_str(&format!("    fn {}({}){};\n", f.c_name(), params, ret));
        }
        out.push_str("}\n\n");

        for f in &go_funcs {
            out.push_str(&gen_rust_go_wrapper(f));
            out.push('\n');
        }
    }

    out
}

fn rust_c_ty(p: &CParam) -> &'static str {
    if p.pointer {
        "*mut i64"
    } else {
        "i64"
    }
}

fn gen_rust_export(f: &FfiFunc) -> String {
    let mut out = String::new();
    let cparams = f.c_params();
    let sig_params = cparams
        .iter()
        .map(|p| format!("{}: {}", p.name, rust_c_ty(p)))
        .collect::<Vec<_>>()
        .join(", ");
    let ret = if f.returns_scalar { " -> i64" } else { "" };

    out.push_str(&format!(
        "#[no_mangle]\npub extern \"C\" fn {}({}){} {{\n",
        f.c_name(),
        sig_params,
        ret
    ));

    // Reconstruct struct locals from the flattened cells.
    let mut call_args = Vec::new();
    for input in &f.inputs {
        match input {
            InputParam::Scalar { name } => call_args.push(name.clone()),
            InputParam::Struct {
                name,
                ty,
                mutated,
                fields,
            } => {
                let mut_kw = if *mutated { "mut " } else { "" };
                let inits = fields
                    .iter()
                    .map(|fld| {
                        let cell = format!("{}_{}", name, fld);
                        let val = if *mutated {
                            format!("unsafe {{ *{} }}", cell)
                        } else {
                            cell
                        };
                        format!("{}: {}", fld, val)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!(
                    "    let {}{} = {} {{ {} }};\n",
                    mut_kw, name, ty, inits
                ));
                if *mutated {
                    call_args.push(format!("&mut {}", name));
                } else {
                    call_args.push(format!("&{}", name));
                }
            }
        }
    }

    let call = format!("{}({})", f.func, call_args.join(", "));
    if f.returns_scalar {
        out.push_str(&format!("    let __ret = {};\n", call));
    } else {
        out.push_str(&format!("    {};\n", call));
    }

    // Write back mutated struct fields.
    for input in &f.inputs {
        if let InputParam::Struct {
            name,
            mutated: true,
            fields,
            ..
        } = input
        {
            for fld in fields {
                let cell = format!("{}_{}", name, fld);
                out.push_str(&format!(
                    "    unsafe {{ *{cell} = {name}.{fld}; }}\n",
                    cell = cell,
                    name = name,
                    fld = fld
                ));
            }
        }
    }

    if f.returns_scalar {
        out.push_str("    __ret\n");
    }
    out.push_str("}\n");
    out
}

fn gen_rust_go_wrapper(f: &FfiFunc) -> String {
    let mut out = String::new();
    let wrapper = format!("call_go_{}", f.func);
    let mut sig_params = Vec::new();
    let mut call_args = Vec::new();
    for input in &f.inputs {
        match input {
            InputParam::Scalar { name } => {
                sig_params.push(format!("{}: i64", name));
                call_args.push(name.clone());
            }
            InputParam::Struct {
                name,
                mutated,
                fields,
                ..
            } => {
                for fld in fields {
                    let cell = format!("{}_{}", name, fld);
                    if *mutated {
                        sig_params.push(format!("{}: &mut i64", cell));
                        call_args.push(format!("{} as *mut i64", cell));
                    } else {
                        sig_params.push(format!("{}: i64", cell));
                        call_args.push(cell);
                    }
                }
            }
        }
    }
    let ret = if f.returns_scalar { " -> i64" } else { "" };
    out.push_str(&format!(
        "/// Safe wrapper: call the Go-generated `{}` from Rust.\n",
        f.func
    ));
    out.push_str(&format!(
        "pub fn {}({}){} {{\n",
        wrapper,
        sig_params.join(", "),
        ret
    ));
    out.push_str(&format!(
        "    unsafe {{ {}({}) }}\n",
        f.c_name(),
        call_args.join(", ")
    ));
    out.push_str("}\n");
    out
}

// -------------------------------------------------------------------- Go side

fn gen_go(funcs: &[FfiFunc], package: &str) -> String {
    let mut out = String::new();
    out.push_str("// Generated by tpt-telos. Automatic FFI bridge (Go side).\n");
    out.push_str("// Go wrappers call the Rust exports via cgo; //export shims expose Go\n");
    out.push_str("// functions to Rust across the same C ABI.\n");
    out.push_str(&format!("package {}\n\n", package));
    out.push_str("/*\n#include <stdint.h>\n#include \"telos_ffi.h\"\n*/\nimport \"C\"\n\n");

    // Calls into Rust.
    let rust_funcs: Vec<&FfiFunc> = funcs.iter().filter(|f| f.target == Target::Rust).collect();
    if !rust_funcs.is_empty() {
        out.push_str("// ===== Calls into Rust (cgo) =====\n\n");
        for f in &rust_funcs {
            out.push_str(&gen_go_call_rust(f));
            out.push('\n');
        }
    }

    // Exports to Rust.
    let go_funcs: Vec<&FfiFunc> = funcs.iter().filter(|f| f.target == Target::Go).collect();
    if !go_funcs.is_empty() {
        out.push_str("// ===== Exports to Rust (cgo //export) =====\n\n");
        for f in &go_funcs {
            out.push_str(&gen_go_export(f));
            out.push('\n');
        }
    }

    crate::go::normalize(out)
}

/// Go signature params mirroring the safe Go function: `*T`/`T` for structs,
/// `int64` for scalars.
fn go_sig_params(f: &FfiFunc) -> Vec<String> {
    let mut params = Vec::new();
    for input in &f.inputs {
        match input {
            InputParam::Scalar { name } => params.push(format!("{} int64", name)),
            InputParam::Struct {
                name, ty, mutated, ..
            } => {
                if *mutated {
                    params.push(format!("{} *{}", name, ty));
                } else {
                    params.push(format!("{} {}", name, ty));
                }
            }
        }
    }
    params
}

fn gen_go_call_rust(f: &FfiFunc) -> String {
    let mut out = String::new();
    let wrapper = format!("CallRust{}", exported(&f.func));
    let ret = if f.returns_scalar { " int64" } else { "" };
    out.push_str(&format!(
        "// {} invokes the Rust-generated {} across the FFI boundary.\n",
        wrapper, f.func
    ));
    out.push_str(&format!(
        "func {}({}){} {{\n",
        wrapper,
        go_sig_params(f).join(", "),
        ret
    ));

    let mut c_args = Vec::new();
    // Prepare cells.
    for input in &f.inputs {
        match input {
            InputParam::Scalar { name } => {
                c_args.push(format!("C.int64_t({})", name));
            }
            InputParam::Struct {
                name,
                mutated,
                fields,
                ..
            } => {
                for fld in fields {
                    let cell = format!("c_{}_{}", name, fld);
                    if *mutated {
                        out.push_str(&format!(
                            "\t{cell} := C.int64_t({name}.{gf})\n",
                            cell = cell,
                            name = name,
                            gf = exported(fld)
                        ));
                        c_args.push(format!("&{}", cell));
                    } else {
                        c_args.push(format!("C.int64_t({}.{})", name, exported(fld)));
                    }
                }
            }
        }
    }

    let call = format!("C.{}({})", f.c_name(), c_args.join(", "));
    if f.returns_scalar {
        out.push_str(&format!("\t__ret := {}\n", call));
    } else {
        out.push_str(&format!("\t{}\n", call));
    }

    // Write back mutated fields.
    for input in &f.inputs {
        if let InputParam::Struct {
            name,
            mutated: true,
            fields,
            ..
        } = input
        {
            for fld in fields {
                out.push_str(&format!(
                    "\t{name}.{gf} = int64(c_{name}_{fld})\n",
                    name = name,
                    gf = exported(fld),
                    fld = fld
                ));
            }
        }
    }

    if f.returns_scalar {
        out.push_str("\treturn int64(__ret)\n");
    }
    out.push_str("}\n");
    out
}

fn gen_go_export(f: &FfiFunc) -> String {
    let mut out = String::new();
    let cparams = f.c_params();
    let sig = cparams
        .iter()
        .map(|p| {
            let ty = if p.pointer { "*C.int64_t" } else { "C.int64_t" };
            format!("{} {}", p.name, ty)
        })
        .collect::<Vec<_>>()
        .join(", ");
    let ret = if f.returns_scalar { " C.int64_t" } else { "" };

    out.push_str(&format!("//export {}\n", f.c_name()));
    out.push_str(&format!("func {}({}){} {{\n", f.c_name(), sig, ret));

    // Reconstruct Go struct locals.
    let mut call_args = Vec::new();
    for input in &f.inputs {
        match input {
            InputParam::Scalar { name } => call_args.push(format!("int64({})", name)),
            InputParam::Struct {
                name,
                ty,
                mutated,
                fields,
            } => {
                let inits = fields
                    .iter()
                    .map(|fld| {
                        let cell = format!("{}_{}", name, fld);
                        let val = if *mutated {
                            format!("int64(*{})", cell)
                        } else {
                            format!("int64({})", cell)
                        };
                        format!("{}: {}", exported(fld), val)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!("\t{} := {}{{{}}}\n", name, ty, inits));
                if *mutated {
                    call_args.push(format!("&{}", name));
                } else {
                    call_args.push(name.clone());
                }
            }
        }
    }

    let call = format!("{}({})", exported(&f.func), call_args.join(", "));
    if f.returns_scalar {
        out.push_str(&format!("\t__ret := {}\n", call));
    } else {
        out.push_str(&format!("\t{}\n", call));
    }

    // Write back mutated fields to the caller's pointers.
    for input in &f.inputs {
        if let InputParam::Struct {
            name,
            mutated: true,
            fields,
            ..
        } = input
        {
            for fld in fields {
                out.push_str(&format!(
                    "\t*{name}_{fld} = C.int64_t({name}.{gf})\n",
                    name = name,
                    fld = fld,
                    gf = exported(fld)
                ));
            }
        }
    }

    if f.returns_scalar {
        out.push_str("\treturn C.int64_t(__ret)\n");
    }
    out.push_str("}\n");
    out
}
