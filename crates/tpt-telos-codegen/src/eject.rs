//! The "eject" hatch for tpt-telos (Phase 4).
//!
//! No abstraction is perfect. Any function may be *ejected* into raw Rust or Go
//! with the `@eject` attribute (or via `telos eject`). The compiler then treats
//! the implementation as a **trusted, opaque block** -- it no longer synthesizes
//! or reasons about the body -- while still **enforcing the outer mathematical
//! contracts at the boundary**.
//!
//! Concretely, for an ejected `func f` we emit two things:
//!   1. `f_impl` / `fImpl` -- the raw, editable opaque implementation.
//!   2. `f` -- a generated boundary guard that asserts every `requires` before
//!      the call and every `ensures` after it (capturing `old(...)` state in
//!      snapshot locals), then delegates to the opaque block.
//!
//! This is the "two-way bridge": telos -> raw code (the opaque block), and raw
//! code -> telos (re-entering behind contract guards).

use tpt_telos_parser::ast::*;

use crate::go::exported;
use crate::{analyze_func, render_expr_doc, render_params_sig, InputParam, TypeFields};

// ================================================================= Rust

/// Render an ejected function as a raw Rust opaque block plus a contract guard.
pub(crate) fn render_rust_ejected(f: &Func, stmts: &[Stmt], types: &TypeFields) -> String {
    let analysis = analyze_func(f, stmts, types);
    let (params, ret) = rust_sig(&analysis);
    let impl_name = format!("{}_impl", f.name);

    let mut out = String::new();
    out.push_str(&format!(
        "// ==== EJECTED (rust): trusted opaque block for `{}`. Edit this freely. ====\n",
        f.name
    ));

    // The opaque implementation: reuse the normal Rust renderer with a renamed,
    // attribute-free clone so we get an identical signature and body.
    let mut impl_f = f.clone();
    impl_f.attributes.clear();
    impl_f.name = impl_name.clone();
    out.push_str(&crate::render_func(&impl_f, stmts, types));
    out.push('\n');

    // The boundary contract guard.
    out.push_str(&format!(
        "// ==== Boundary contract guard for `{}` (generated; do not edit). ====\n",
        f.name
    ));
    out.push_str(&format!("/// {}({})\n", f.name, render_params_sig(f)));
    for r in &f.requires {
        out.push_str(&format!("/// requires: {}\n", render_expr_doc(r)));
    }
    for e in &f.ensures {
        out.push_str(&format!("/// ensures:  {}\n", render_expr_doc(e)));
    }
    out.push_str(&format!(
        "pub fn {}({}){} {{\n",
        f.name,
        params.join(", "),
        ret
    ));

    for r in &f.requires {
        out.push_str(&format!(
            "    assert!({}, \"telos: precondition violated in `{}`: {}\");\n",
            rust_guard_expr(r, analysis.scalar_out.as_deref(), false),
            f.name,
            escape(&render_expr_doc(r))
        ));
    }

    let olds = collect_old_fields(&f.ensures);
    for (base, field) in &olds {
        out.push_str(&format!(
            "    let __old_{}_{} = {}.{};\n",
            base, field, base, field
        ));
    }

    let call_args = call_args(&analysis);
    if analysis.scalar_out.is_some() {
        out.push_str(&format!("    let __ret = {}({});\n", impl_name, call_args));
    } else {
        out.push_str(&format!("    {}({});\n", impl_name, call_args));
    }

    for e in &f.ensures {
        out.push_str(&format!(
            "    assert!({}, \"telos: postcondition violated in `{}`: {}\");\n",
            rust_guard_expr(e, analysis.scalar_out.as_deref(), true),
            f.name,
            escape(&render_expr_doc(e))
        ));
    }

    if analysis.scalar_out.is_some() {
        out.push_str("    __ret\n");
    }
    out.push_str("}\n");
    out
}

fn rust_sig(analysis: &crate::FuncAnalysis) -> (Vec<String>, String) {
    let mut params = Vec::new();
    for input in &analysis.inputs {
        match input {
            InputParam::Scalar { name } => params.push(format!("{}: i64", name)),
            InputParam::Struct {
                name, ty, mutated, ..
            } => {
                let borrow = if *mutated { "&mut " } else { "&" };
                params.push(format!("{}: {}{}", name, borrow, ty));
            }
        }
    }
    let ret = if analysis.scalar_out.is_some() {
        " -> i64".to_string()
    } else {
        String::new()
    };
    (params, ret)
}

fn rust_guard_expr(e: &Expr, scalar_out: Option<&str>, post: bool) -> String {
    rust_guard_inner(e, scalar_out, post, false)
}

fn rust_guard_inner(e: &Expr, scalar_out: Option<&str>, post: bool, in_old: bool) -> String {
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Var(v) => {
            if post && !in_old && scalar_out == Some(v.as_str()) {
                "__ret".to_string()
            } else {
                v.clone()
            }
        }
        Expr::Field { base, field } => {
            if in_old {
                format!("__old_{}_{}", base, field)
            } else {
                format!("{}.{}", base, field)
            }
        }
        Expr::Old(inner) => rust_guard_inner(inner, scalar_out, post, true),
        Expr::Unary { op, expr } => match op {
            UnOp::Neg => format!("-{}", rust_guard_inner(expr, scalar_out, post, in_old)),
        },
        Expr::Bin { op, lhs, rhs } => format!(
            "{} {} {}",
            rust_guard_inner(lhs, scalar_out, post, in_old),
            crate::go::bin_op(*op),
            rust_guard_inner(rhs, scalar_out, post, in_old)
        ),
        // New expression kinds: fall back to standard rendering.
        other => crate::render_expr(other),
    }
}

// ================================================================= Go

/// Render an ejected function as a raw Go opaque block plus a contract guard.
pub(crate) fn render_go_ejected(f: &Func, stmts: &[Stmt], types: &TypeFields) -> String {
    let analysis = analyze_func(f, stmts, types);
    let impl_name = format!("{}Impl", lower_first(&f.name));
    let guard_name = exported(&f.name);

    let mut out = String::new();
    out.push_str(&format!(
        "// ==== EJECTED (go): trusted opaque block for `{}`. Edit this freely. ====\n",
        f.name
    ));
    out.push_str(&crate::go::render_func_named(f, stmts, types, &impl_name));
    out.push('\n');

    out.push_str(&format!(
        "// ==== Boundary contract guard for `{}` (generated; do not edit). ====\n",
        f.name
    ));
    for r in &f.requires {
        out.push_str(&format!("// requires: {}\n", render_expr_doc(r)));
    }
    for e in &f.ensures {
        out.push_str(&format!("// ensures:  {}\n", render_expr_doc(e)));
    }

    let (params, ret) = go_sig(&analysis);
    out.push_str(&format!(
        "func {}({}){} {{\n",
        guard_name,
        params.join(", "),
        ret
    ));

    for r in &f.requires {
        out.push_str(&format!(
            "\tif !({}) {{\n\t\tpanic(\"telos: precondition violated in {}: {}\")\n\t}}\n",
            go_guard_expr(r, analysis.scalar_out.as_deref(), false),
            f.name,
            escape(&render_expr_doc(r))
        ));
    }

    let olds = collect_old_fields(&f.ensures);
    for (base, field) in &olds {
        out.push_str(&format!(
            "\t__old_{}_{} := {}.{}\n",
            base,
            field,
            base,
            exported(field)
        ));
    }

    let call_args = call_args(&analysis);
    if analysis.scalar_out.is_some() {
        out.push_str(&format!("\t__ret := {}({})\n", impl_name, call_args));
    } else {
        out.push_str(&format!("\t{}({})\n", impl_name, call_args));
    }

    for e in &f.ensures {
        out.push_str(&format!(
            "\tif !({}) {{\n\t\tpanic(\"telos: postcondition violated in {}: {}\")\n\t}}\n",
            go_guard_expr(e, analysis.scalar_out.as_deref(), true),
            f.name,
            escape(&render_expr_doc(e))
        ));
    }

    if analysis.scalar_out.is_some() {
        out.push_str("\treturn __ret\n");
    }
    out.push_str("}\n");
    out
}

fn go_sig(analysis: &crate::FuncAnalysis) -> (Vec<String>, String) {
    let mut params = Vec::new();
    for input in &analysis.inputs {
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
    let ret = if analysis.scalar_out.is_some() {
        " int64".to_string()
    } else {
        String::new()
    };
    (params, ret)
}

fn go_guard_expr(e: &Expr, scalar_out: Option<&str>, post: bool) -> String {
    go_guard_inner(e, scalar_out, post, false)
}

fn go_guard_inner(e: &Expr, scalar_out: Option<&str>, post: bool, in_old: bool) -> String {
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Var(v) => {
            if post && !in_old && scalar_out == Some(v.as_str()) {
                "__ret".to_string()
            } else {
                v.clone()
            }
        }
        Expr::Field { base, field } => {
            if in_old {
                format!("__old_{}_{}", base, field)
            } else {
                format!("{}.{}", base, exported(field))
            }
        }
        Expr::Old(inner) => go_guard_inner(inner, scalar_out, post, true),
        Expr::Unary { op, expr } => match op {
            UnOp::Neg => format!("-{}", go_guard_inner(expr, scalar_out, post, in_old)),
        },
        Expr::Bin { op, lhs, rhs } => format!(
            "{} {} {}",
            go_guard_inner(lhs, scalar_out, post, in_old),
            crate::go::bin_op(*op),
            go_guard_inner(rhs, scalar_out, post, in_old)
        ),
        other => crate::go::render_expr(other),
    }
}

// ================================================================= shared

fn call_args(analysis: &crate::FuncAnalysis) -> String {
    analysis
        .inputs
        .iter()
        .map(|i| match i {
            InputParam::Scalar { name } => name.clone(),
            InputParam::Struct { name, .. } => name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Collect every `(base, field)` referenced inside an `old(...)` in the given
/// clauses, so the guard can snapshot them before the opaque call.
fn collect_old_fields(clauses: &[Expr]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for c in clauses {
        walk_old(c, false, &mut out);
    }
    out.dedup();
    out
}

fn walk_old(e: &Expr, in_old: bool, out: &mut Vec<(String, String)>) {
    match e {
        Expr::Field { base, field } => {
            if in_old {
                let pair = (base.clone(), field.clone());
                if !out.contains(&pair) {
                    out.push(pair);
                }
            }
        }
        Expr::Old(inner) => walk_old(inner, true, out),
        Expr::Unary { expr, .. } => walk_old(expr, in_old, out),
        Expr::Bin { lhs, rhs, .. } => {
            walk_old(lhs, in_old, out);
            walk_old(rhs, in_old, out);
        }
        _ => {}
    }
}

fn lower_first(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => c.to_ascii_lowercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
