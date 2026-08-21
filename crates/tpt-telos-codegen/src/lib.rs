//! Rust backend code generation for tpt-telos (Phase 2 milestone).
//!
//! [`generate_program`] lowers a set of parsed modules into a single,
//! self-contained Rust library source string. The generated code:
//!   * declares a `struct` for every user type that is accessed by field,
//!   * attaches an `invariants()` method to each such struct for every
//!     `invariant` block of the same name,
//!   * emits one `fn` per `func`, threading `&mut` only to parameters that are
//!     actually mutated,
//!   * carries the original `requires` / `ensures` contracts as doc-comments.
//!
//! The function bodies come from the agentic transpiler's final candidates
//! (see [`tpt_telos_agent`]), so the emitted code is mathematically verified before
//! it is written out.

use std::collections::{BTreeSet, HashMap, HashSet};

use tpt_telos_agent::FuncOutcome;
use tpt_telos_parser::ast::*;

pub mod eject;
pub mod ffi;
pub mod go;
pub mod project;
pub mod proof;
pub mod python;

pub use project::{generate_project, GeneratedFile, Project};

/// Maps a type name to the set of fields that must appear on its generated
/// struct.
/// Per-type field definitions: maps a type name to its ordered list of
/// `(field_name, field_type)` pairs. Populated from explicit `StructDef`
/// declarations when available; otherwise inferred from usage.
pub(crate) type TypeFields = HashMap<String, Vec<(String, Type)>>;

/// Classification of a single (post-analysis) function parameter, shared by the
/// Rust backend, the Go backend, and the automatic FFI bridge so all three
/// agree on calling conventions.
#[derive(Debug, Clone)]
pub(crate) enum InputParam {
    /// A scalar (`i64`) input parameter.
    Scalar { name: String },
    /// A parameter of a struct type. `mutated` is true when the function body
    /// writes to any of its fields (so it is passed `&mut` in Rust and by
    /// pointer across the FFI boundary).
    Struct {
        name: String,
        ty: String,
        mutated: bool,
        fields: Vec<String>,
    },
}

/// The result of analysing a function body: its effective input parameters and
/// whether it produces a scalar return value (an assignment to a bare
/// parameter/variable name).
#[derive(Debug, Clone)]
pub(crate) struct FuncAnalysis {
    pub inputs: Vec<InputParam>,
    pub scalar_out: Option<String>,
}

/// Analyse a function's parameters and body once, deriving the mutation set and
/// scalar-return output so every backend lowers the same signature.
pub(crate) fn analyze_func(f: &Func, stmts: &[Stmt], types: &TypeFields) -> FuncAnalysis {
    let mut mutated: HashSet<String> = HashSet::new();
    let mut scalar_out: Option<String> = None;
    for stmt in stmts {
        let assigns = match stmt {
            Stmt::MutateState(a) => a,
            Stmt::Assign(a) => std::slice::from_ref(a),
            _ => continue,
        };
        for a in assigns {
            match &a.target {
                Expr::Field { base, .. } => {
                    mutated.insert(base.clone());
                }
                Expr::Var(v) if scalar_out.is_none() => {
                    scalar_out = Some(v.clone());
                }
                _ => {}
            }
        }
    }

    let mut inputs = Vec::new();
    for p in &f.params {
        if Some(&p.name) == scalar_out.as_ref() {
            continue; // produced as the return value, not an input
        }
        let ty = p.ty.name();
        let struct_fields: Vec<String> = types
            .get(ty)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        if struct_fields.is_empty() {
            inputs.push(InputParam::Scalar {
                name: p.name.clone(),
            });
        } else {
            inputs.push(InputParam::Struct {
                name: p.name.clone(),
                ty: ty.to_string(),
                mutated: mutated.contains(&p.name),
                fields: struct_fields.into_iter().collect(),
            });
        }
    }

    FuncAnalysis { inputs, scalar_out }
}

/// Generate a full Rust library source for all modules.
///
/// `bodies` supplies the final (verified) body for each function, keyed by
/// `"<module>.<func>"`. A function absent from the map falls back to its body
/// in the parsed source.
///
/// # Examples
///
/// ```
/// use tpt_telos_parser::parse;
/// use tpt_telos_agent::{StaticAgent, transpile_module};
/// use tpt_telos_codegen::generate_program;
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
/// let agent = StaticAgent::new();
/// let outcomes: Vec<_> = modules.iter()
///     .flat_map(|m| transpile_module(m, &agent).unwrap())
///     .collect();
///
/// let rust_src = generate_program(&modules, &outcomes);
///
/// assert!(rust_src.contains("pub struct Wallet"));
/// assert!(rust_src.contains("pub fn deposit"));
/// ```
pub fn generate_program(modules: &[Module], outcomes: &[FuncOutcome]) -> String {
    let bodies = collect_bodies(outcomes);
    let refs: Vec<&Module> = modules.iter().collect();
    render_rust(&refs, &bodies)
}

/// Render a Rust library for the given modules using the supplied verified
/// bodies. Shared by [`generate_program`] and the project assembler so both
/// emit identical Rust.
pub(crate) fn render_rust(modules: &[&Module], bodies: &HashMap<String, Vec<Stmt>>) -> String {
    let mut out = String::new();
    out.push_str("// Generated by tpt-telos. Do not edit by hand.\n");
    out.push_str("// Mathematical contracts were verified by the tpt-telos SMT core.\n");
    out.push_str("#![allow(dead_code)]\n\n");

    let mut struct_types: TypeFields = HashMap::new();
    for m in modules {
        collect_types(m, &mut struct_types);
    }

    for m in modules {
        out.push_str(&generate_module(m, bodies, &struct_types));
        out.push('\n');
    }

    out
}

/// Collect the final (verified) body for each function, keyed by function name.
pub(crate) fn collect_bodies(outcomes: &[FuncOutcome]) -> HashMap<String, Vec<Stmt>> {
    let mut bodies: HashMap<String, Vec<Stmt>> = HashMap::new();
    for o in outcomes {
        bodies.insert(o.func_name.clone(), o.final_candidate.stmts.clone());
    }
    bodies
}

pub(crate) fn collect_types(module: &Module, types: &mut TypeFields) {
    // First pass: collect explicit StructDef declarations — these provide
    // the authoritative type information for each field.
    for item in &module.items {
        if let Item::Struct(sd) = item {
            let entry = types.entry(sd.name.clone()).or_default();
            for fd in &sd.fields {
                // Don't overwrite if already present (preserve order from first def).
                if !entry.iter().any(|(n, _)| n == &fd.name) {
                    entry.push((fd.name.clone(), fd.ty.clone()));
                }
            }
        }
        if let Item::Enum(ed) = item {
            // Enum variants become unit variants in codegen; record the enum
            // name with no fields (the router/codegen handles enum-specific logic).
            types.entry(ed.name.clone()).or_default();
        }
    }

    // Second pass: infer fields from usage for types without explicit StructDef.
    let mut raw_fields: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut field_users: HashSet<String> = HashSet::new();

    let scan_expr =
        |e: &Expr, users: &mut HashSet<String>, raw: &mut HashMap<String, BTreeSet<String>>| {
            let mut fields: Vec<(String, String)> = Vec::new();
            collect_fields(e, &mut fields);
            for (base, field) in fields {
                users.insert(base.clone());
                raw.entry(base.clone()).or_default().insert(field);
            }
        };

    for item in &module.items {
        if let Item::Func(f) = item {
            for r in &f.requires {
                scan_expr(r, &mut field_users, &mut raw_fields);
            }
            for e in &f.ensures {
                scan_expr(e, &mut field_users, &mut raw_fields);
            }
            for stmt in &f.body {
                let assigns = match stmt {
                    Stmt::MutateState(a) => a,
                    Stmt::Assign(a) => std::slice::from_ref(a),
                    _ => continue,
                };
                for a in assigns {
                    if let Expr::Field { base, field } = &a.target {
                        field_users.insert(base.clone());
                        raw_fields
                            .entry(base.clone())
                            .or_default()
                            .insert(field.clone());
                    }
                    scan_expr(&a.value, &mut field_users, &mut raw_fields);
                }
            }
        }
    }

    // Re-point: a base that is a parameter of a named type is really that
    // *type's* field.
    for item in &module.items {
        if let Item::Func(f) = item {
            for p in &f.params {
                if let Some(fields) = raw_fields.remove(&p.name) {
                    let ty_name = p.ty.name().to_string();
                    let entry = types.entry(ty_name).or_default();
                    for f in fields {
                        if !entry.iter().any(|(n, _)| n == &f) {
                            // Infer type as Int for usage-inferred fields.
                            entry.push((f, Type::int()));
                        }
                    }
                }
            }
        }
    }
    // Any bases that were never parameters keep their raw (base-named) fields.
    for (base, fields) in raw_fields {
        let entry = types.entry(base).or_default();
        for f in fields {
            if !entry.iter().any(|(n, _)| n == &f) {
                entry.push((f, Type::int()));
            }
        }
    }
}

fn collect_fields(expr: &Expr, out: &mut Vec<(String, String)>) {
    match expr {
        Expr::Field { base, field } => out.push((base.clone(), field.clone())),
        Expr::Old(e) => collect_fields(e, out),
        Expr::Unary { expr, .. } => collect_fields(expr, out),
        Expr::Bin { lhs, rhs, .. } => {
            collect_fields(lhs, out);
            collect_fields(rhs, out);
        }
        _ => {}
    }
}

fn generate_module(
    module: &Module,
    bodies: &HashMap<String, Vec<Stmt>>,
    types: &TypeFields,
) -> String {
    let route = tpt_telos_router::route(&module.attributes);
    let is_persistent = route.storage == tpt_telos_router::StorageClass::Persistent;
    let mut out = String::new();
    out.push_str(&format!(
        "// ===== module {} (target: {}, storage: {}) =====\n",
        module.name,
        route.target.as_str(),
        if is_persistent {
            "persistent"
        } else {
            "ephemeral"
        }
    ));

    // Structs.
    for (ty, fields) in types {
        if fields.is_empty() {
            continue;
        }
        if is_persistent {
            out.push_str("#[derive(serde::Serialize, serde::Deserialize)]\n");
        }
        out.push_str(&format!("pub struct {} {{\n", ty));
        for (f, fty) in fields {
            out.push_str(&format!("    pub {}: {},\n", f, render_type(fty)));
        }
        out.push_str("}\n\n");
    }

    // Invariants as `impl` methods.
    for item in &module.items {
        if let Item::Invariant(inv) = item {
            let conds = inv
                .constraints
                .iter()
                .map(render_inv)
                .collect::<Vec<_>>()
                .join(" && ");
            out.push_str(&format!(
                "impl {} {{\n    pub fn satisfies_invariants(&self) -> bool {{\n        {}\n    }}\n}}\n\n",
                inv.name, conds
            ));
        }
    }

    // Functions.
    for item in &module.items {
        if let Item::Func(f) = item {
            let key = f.name.clone();
            let stmts = bodies.get(&key).cloned().unwrap_or_else(|| f.body.clone());
            if f.is_ejected() {
                out.push_str(&eject::render_rust_ejected(f, &stmts, types));
            } else {
                out.push_str(&render_func(f, &stmts, types));
            }
            out.push('\n');
        }
    }

    out
}

pub(crate) fn render_func(f: &Func, stmts: &[Stmt], types: &TypeFields) -> String {
    let mut out = String::new();

    // Doc-comment carrying the contract.
    out.push_str(&format!("/// {}({})\n", f.name, render_params_sig(f)));
    for r in &f.requires {
        out.push_str(&format!("/// requires: {}\n", render_expr_doc(r)));
    }
    for e in &f.ensures {
        out.push_str(&format!("/// ensures:  {}\n", render_expr_doc(e)));
    }

    // Determine mutated struct params and scalar-return outputs via the shared
    // analysis so the Rust, Go, and FFI backends agree on the signature.
    let analysis = analyze_func(f, stmts, types);
    let scalar_out = analysis.scalar_out.clone();

    // Build parameter list + return type.
    let mut params = Vec::new();
    for input in &analysis.inputs {
        match input {
            InputParam::Scalar { name } => {
                params.push(format!("{}: i64", name));
            }
            InputParam::Struct {
                name, ty, mutated, ..
            } => {
                let borrow = if *mutated { "&mut " } else { "&" };
                params.push(format!("{}: {}{}", name, borrow, ty));
            }
        }
    }

    let uses_try = stmts.iter().any(stmt_contains_try);
    let ret = if uses_try {
        if scalar_out.is_some() {
            " -> Result<i64, String>".to_string()
        } else {
            " -> Result<(), String>".to_string()
        }
    } else if scalar_out.is_some() {
        " -> i64".to_string()
    } else {
        String::new()
    };

    out.push_str(&format!(
        "pub fn {}({}){} {{\n",
        f.name,
        params.join(", "),
        ret
    ));

    // Body.
    for stmt in stmts {
        match stmt {
            Stmt::MutateState(assigns) => {
                for a in assigns {
                    out.push_str("    ");
                    out.push_str(&render_assign(a));
                    out.push_str(";\n");
                }
            }
            Stmt::Assign(a) => {
                let value = render_expr(&a.value);
                if scalar_out.as_deref() == Some(target_var(&a.target)) {
                    out.push_str(&format!("    let {} = {};\n", target_var(&a.target), value));
                } else {
                    out.push_str("    ");
                    out.push_str(&render_assign(a));
                    out.push_str(";\n");
                }
            }
            Stmt::Let(lb) => {
                let ty = lb
                    .ty
                    .as_ref()
                    .map(|t| format!(": {}", render_type(t)))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "    let {}{} = {};\n",
                    lb.name,
                    ty,
                    render_expr(&lb.value)
                ));
            }
            Stmt::If(is) => {
                out.push_str(&format!("    if {} {{\n", render_expr(&is.condition)));
                for s in &is.then_body {
                    render_stmt_indented(s, &mut out, 2);
                }
                out.push_str("    }");
                if let Some(else_body) = &is.else_body {
                    out.push_str(" else {\n");
                    for s in else_body {
                        render_stmt_indented(s, &mut out, 2);
                    }
                    out.push_str("    }");
                }
                out.push('\n');
            }
            Stmt::Match(ms) => {
                out.push_str(&format!("    match {} {{\n", render_expr(&ms.scrutinee)));
                for arm in &ms.arms {
                    out.push_str(&format!("        {} => {{\n", render_pattern(&arm.pattern)));
                    for s in &arm.body {
                        render_stmt_indented(s, &mut out, 3);
                    }
                    out.push_str("        }\n");
                }
                out.push_str("    }\n");
            }
            Stmt::Return(e) => match e {
                Some(expr) => out.push_str(&format!("    return {};\n", render_expr(expr))),
                None => out.push_str("    return;\n"),
            },
        }
    }

    if let Some(outvar) = &scalar_out {
        if uses_try {
            out.push_str(&format!("    Ok({})\n", outvar));
        } else {
            out.push_str(&format!("    {}\n", outvar));
        }
    }

    out.push_str("}\n");
    out
}

fn stmt_contains_try(s: &Stmt) -> bool {
    match s {
        Stmt::MutateState(assigns) => assigns.iter().any(|a| expr_contains_try(&a.value)),
        Stmt::Assign(a) => expr_contains_try(&a.value),
        Stmt::Let(lb) => expr_contains_try(&lb.value),
        Stmt::If(is) => {
            expr_contains_try(&is.condition)
                || is.then_body.iter().any(stmt_contains_try)
                || is
                    .else_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(stmt_contains_try))
        }
        Stmt::Match(ms) => {
            expr_contains_try(&ms.scrutinee)
                || ms.arms.iter().any(|a| a.body.iter().any(stmt_contains_try))
        }
        Stmt::Return(e) => e.as_ref().is_some_and(expr_contains_try),
    }
}

fn expr_contains_try(e: &Expr) -> bool {
    match e {
        Expr::Try(_) => true,
        Expr::Unary { expr, .. } => expr_contains_try(expr),
        Expr::Bin { lhs, rhs, .. } => expr_contains_try(lhs) || expr_contains_try(rhs),
        Expr::Call(c) => c.args.iter().any(expr_contains_try),
        Expr::MethodCall(m) => {
            expr_contains_try(&m.receiver) || m.args.iter().any(expr_contains_try)
        }
        Expr::Index(i) => expr_contains_try(&i.receiver) || expr_contains_try(&i.index),
        Expr::If(i) => {
            expr_contains_try(&i.condition)
                || expr_contains_try(&i.then_expr)
                || expr_contains_try(&i.else_expr)
        }
        Expr::Match(m) => {
            expr_contains_try(&m.scrutinee) || m.arms.iter().any(|a| expr_contains_try(&a.expr))
        }
        Expr::Old(e) => expr_contains_try(e),
        Expr::Forall(f) => expr_contains_try(&f.body),
        Expr::Aggregate(a) => a.args.iter().any(expr_contains_try),
        _ => false,
    }
}

pub(crate) fn render_params_sig(f: &Func) -> String {
    f.params
        .iter()
        .map(|p| format!("{}: {}", p.name, p.ty.name()))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn target_var(e: &Expr) -> &str {
    match e {
        Expr::Var(v) => v,
        _ => "",
    }
}

pub(crate) fn render_assign(a: &Assign) -> String {
    let lhs = render_expr(&a.target);
    let rhs = render_expr(&a.value);
    let op = match a.op {
        AssignOp::Set => "=",
        AssignOp::Add => "+=",
        AssignOp::Sub => "-=",
    };
    format!("{} {} {}", lhs, op, rhs)
}

pub(crate) fn render_expr(e: &Expr) -> String {
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Var(v) => v.clone(),
        Expr::Field { base, field } => format!("{}.{}", base, field),
        Expr::Old(inner) => render_expr(inner),
        Expr::Unary { op, expr } => match op {
            UnOp::Neg => format!("-{}", render_expr(expr)),
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
            format!("{} {} {}", render_expr(lhs), s, render_expr(rhs))
        }
        Expr::Call(c) => {
            let args: Vec<_> = c.args.iter().map(render_expr).collect();
            format!("{}({})", c.func, args.join(", "))
        }
        Expr::MethodCall(m) => {
            let args: Vec<_> = m.args.iter().map(render_expr).collect();
            format!(
                "{}.{}({})",
                render_expr(&m.receiver),
                m.method,
                args.join(", ")
            )
        }
        Expr::Index(i) => {
            format!("{}[{}]", render_expr(&i.receiver), render_expr(&i.index))
        }
        Expr::If(i) => format!(
            "if {} {{ {} }} else {{ {} }}",
            render_expr(&i.condition),
            render_expr(&i.then_expr),
            render_expr(&i.else_expr)
        ),
        Expr::Match(m) => {
            let arms: Vec<_> = m
                .arms
                .iter()
                .map(|a| format!("{} => {}", render_pattern(&a.pattern), render_expr(&a.expr)))
                .collect();
            format!(
                "match {} {{ {} }}",
                render_expr(&m.scrutinee),
                arms.join(", ")
            )
        }
        Expr::Try(e) => format!("{}?", render_expr(e)),
        Expr::Forall(f) => format!(
            "forall {}: {} {{ {} }}",
            f.var,
            render_type(&f.var_ty),
            render_expr(&f.body)
        ),
        Expr::Aggregate(a) => {
            let args: Vec<_> = a.args.iter().map(render_expr).collect();
            let op = match a.op {
                AggregateOp::Sum => "sum",
                AggregateOp::Min => "min",
                AggregateOp::Max => "max",
                AggregateOp::Count => "count",
            };
            format!("{}({})", op, args.join(", "))
        }
        Expr::Range { lo, hi } => format!("{}..{}", render_expr(lo), render_expr(hi)),
    }
}

/// Like [`render_expr`] but preserves `old(...)` so contracts read faithfully in
/// doc-comments.
pub(crate) fn render_expr_doc(e: &Expr) -> String {
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Var(v) => v.clone(),
        Expr::Field { base, field } => format!("{}.{}", base, field),
        Expr::Old(inner) => format!("old({})", render_expr_doc(inner)),
        Expr::Unary { op, expr } => match op {
            UnOp::Neg => format!("-{}", render_expr_doc(expr)),
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
            format!("{} {} {}", render_expr_doc(lhs), s, render_expr_doc(rhs))
        }
        // For doc-comments, fall back to the standard renderer for new kinds.
        other => render_expr(other),
    }
}

/// Render an expression inside an `impl` method body, prefixing field/var
/// references with `self.` (invariants are written against the struct's own
/// fields).
fn render_inv(e: &Expr) -> String {
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Var(v) => format!("self.{}", v),
        Expr::Field { field, .. } => format!("self.{}", field),
        Expr::Old(inner) => render_inv(inner),
        Expr::Unary { op, expr } => match op {
            UnOp::Neg => format!("-{}", render_inv(expr)),
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
            format!("{} {} {}", render_inv(lhs), s, render_inv(rhs))
        }
        other => render_expr(other),
    }
}

pub(crate) fn render_type(t: &Type) -> String {
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

pub(crate) fn render_pattern(p: &Pattern) -> String {
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

fn render_stmt_indented(s: &Stmt, out: &mut String, indent: usize) {
    let prefix = "    ".repeat(indent);
    let rendered = match s {
        Stmt::MutateState(assigns) => {
            let inner: Vec<_> = assigns.iter().map(render_assign).collect();
            format!("mutate state {{ {} }}", inner.join("; "))
        }
        Stmt::Assign(a) => render_assign(a),
        Stmt::Let(lb) => {
            let ty = lb
                .ty
                .as_ref()
                .map(|t| format!(": {}", render_type(t)))
                .unwrap_or_default();
            format!("let {}{} = {};", lb.name, ty, render_expr(&lb.value))
        }
        Stmt::If(is) => {
            let mut result = format!("if {} {{\n", render_expr(&is.condition));
            for s in &is.then_body {
                result.push_str(&format!("{}    ", prefix));
                render_stmt_indented(s, &mut result, indent + 1);
            }
            result.push_str(&format!("{}}}", prefix));
            if let Some(else_body) = &is.else_body {
                result.push_str(" else {\n");
                for s in else_body {
                    result.push_str(&format!("{}    ", prefix));
                    render_stmt_indented(s, &mut result, indent + 1);
                }
                result.push_str(&format!("{}}}", prefix));
            }
            result
        }
        Stmt::Match(ms) => {
            let mut result = format!("match {} {{\n", render_expr(&ms.scrutinee));
            for arm in &ms.arms {
                result.push_str(&format!(
                    "{}    {} => {{\n",
                    prefix,
                    render_pattern(&arm.pattern)
                ));
                for s in &arm.body {
                    result.push_str(&format!("{}        ", prefix));
                    render_stmt_indented(s, &mut result, indent + 2);
                }
                result.push_str(&format!("{}}}\n", prefix));
            }
            result.push_str(&format!("{}}}", prefix));
            result
        }
        Stmt::Return(e) => match e {
            Some(expr) => format!("return {};", render_expr(expr)),
            None => "return;".to_string(),
        },
    };
    out.push_str(&format!("{}{}\n", prefix, rendered));
}

// ===========================================================================
// Unit tests for the individual codegen pieces (struct mutability, invariant
// `impl` generation, doc-comment emission, the shared `analyze_func`, type
// collection, and the eject hatch). These exercise the generators in isolation
// from the full `generate_program` / `generate_project` pipeline tests in
// `tests/gen.rs`.
// ===========================================================================

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tpt_telos_parser::ast::*;
    use tpt_telos_parser::Span;

    use crate::{
        analyze_func, collect_types, eject, generate_program, go, render_func, InputParam,
        TypeFields,
    };

    fn t_var(v: &str) -> Expr {
        Expr::Var(v.to_string())
    }
    fn t_int(n: i64) -> Expr {
        Expr::Int(n)
    }
    fn t_field(base: &str, f: &str) -> Expr {
        Expr::Field {
            base: base.to_string(),
            field: f.to_string(),
        }
    }
    fn t_old(e: Expr) -> Expr {
        Expr::Old(Box::new(e))
    }
    fn t_bin(op: BinOp, l: Expr, r: Expr) -> Expr {
        Expr::Bin {
            op,
            lhs: Box::new(l),
            rhs: Box::new(r),
        }
    }
    fn t_assign(target: Expr, op: AssignOp, value: Expr) -> Assign {
        Assign { target, op, value }
    }
    fn t_func(
        name: &str,
        params: Vec<Param>,
        requires: Vec<Expr>,
        ensures: Vec<Expr>,
        body: Vec<Stmt>,
    ) -> Func {
        Func {
            attributes: vec![],
            name: name.to_string(),
            params,
            return_ty: None,
            requires,
            ensures,
            body,
            elided: false,
            span: Span::default(),
            requires_spans: vec![],
            ensures_spans: vec![],
        }
    }

    fn counter_types() -> TypeFields {
        let mut m = HashMap::new();
        m.insert("Counter".to_string(), vec![("v".to_string(), Type::int())]);
        m
    }

    // ---- analyze_func ----

    #[test]
    fn analyze_scalar_param() {
        let f = t_func(
            "f",
            vec![Param {
                name: "x".into(),
                ty: Type::Named("i64".into()),
                mutability: ParamMutability::Immutable,
            }],
            vec![],
            vec![],
            vec![],
        );
        let a = analyze_func(&f, &f.body, &HashMap::new());
        assert!(matches!(a.inputs[0], InputParam::Scalar { .. }));
        assert!(a.scalar_out.is_none());
    }

    #[test]
    fn analyze_struct_param_mutated_is_borrow_mut() {
        let f = t_func(
            "f",
            vec![Param {
                name: "c".into(),
                ty: Type::Named("Counter".into()),
                mutability: ParamMutability::Immutable,
            }],
            vec![],
            vec![],
            vec![Stmt::MutateState(vec![t_assign(
                t_field("c", "v"),
                AssignOp::Set,
                t_int(0),
            )])],
        );
        let a = analyze_func(&f, &f.body, &counter_types());
        match &a.inputs[0] {
            InputParam::Struct { mutated, .. } => assert!(*mutated),
            other => panic!("expected struct param, got {other:?}"),
        }
    }

    #[test]
    fn analyze_struct_param_unmutated_is_borrow_ref() {
        let f = t_func(
            "f",
            vec![Param {
                name: "c".into(),
                ty: Type::Named("Counter".into()),
                mutability: ParamMutability::Immutable,
            }],
            vec![],
            vec![],
            vec![],
        );
        let a = analyze_func(&f, &f.body, &counter_types());
        match &a.inputs[0] {
            InputParam::Struct { mutated, .. } => assert!(!*mutated),
            other => panic!("expected struct param, got {other:?}"),
        }
    }

    #[test]
    fn analyze_scalar_return_value() {
        // A bare assignment to a parameter-named variable becomes the scalar
        // return; that parameter is then excluded from the inputs.
        let f = t_func(
            "f",
            vec![
                Param {
                    name: "c".into(),
                    ty: Type::Named("Counter".into()),
                    mutability: ParamMutability::Immutable,
                },
                Param {
                    name: "result".into(),
                    ty: Type::Named("i64".into()),
                    mutability: ParamMutability::Immutable,
                },
            ],
            vec![],
            vec![],
            vec![Stmt::Assign(t_assign(
                t_var("result"),
                AssignOp::Set,
                t_bin(BinOp::Add, t_field("c", "v"), t_int(1)),
            ))],
        );
        let a = analyze_func(&f, &f.body, &counter_types());
        // `result` is the return, so only `c` remains as an input.
        assert_eq!(a.inputs.len(), 1);
        assert_eq!(a.scalar_out.as_deref(), Some("result"));
    }

    // ---- render_func ----

    #[test]
    fn render_func_emits_contract_doc_comments() {
        let f = t_func(
            "transfer",
            vec![Param {
                name: "c".into(),
                ty: Type::Named("Counter".into()),
                mutability: ParamMutability::Immutable,
            }],
            vec![t_bin(BinOp::Ge, t_field("c", "v"), t_int(0))],
            vec![t_bin(
                BinOp::Eq,
                t_field("c", "v"),
                t_bin(BinOp::Add, t_old(t_field("c", "v")), t_int(1)),
            )],
            vec![Stmt::MutateState(vec![t_assign(
                t_field("c", "v"),
                AssignOp::Set,
                t_bin(BinOp::Add, t_field("c", "v"), t_int(1)),
            )])],
        );
        let out = render_func(&f, &f.body, &counter_types());
        assert!(out.contains("/// requires:"), "missing requires doc: {out}");
        assert!(out.contains("/// ensures:"), "missing ensures doc: {out}");
        // The `old(...)` must be preserved faithfully in the doc comment.
        assert!(out.contains("old(c.v)"), "old() lost in doc: {out}");
        // The mutated struct param is threaded as `&mut`.
        assert!(
            out.contains("c: &mut Counter"),
            "expected &mut param: {out}"
        );
    }

    #[test]
    fn render_func_emits_scalar_return() {
        let f = t_func(
            "f",
            vec![
                Param {
                    name: "c".into(),
                    ty: Type::Named("Counter".into()),
                    mutability: ParamMutability::Immutable,
                },
                Param {
                    name: "result".into(),
                    ty: Type::Named("i64".into()),
                    mutability: ParamMutability::Immutable,
                },
            ],
            vec![],
            vec![],
            vec![Stmt::Assign(t_assign(
                t_var("result"),
                AssignOp::Set,
                t_bin(BinOp::Add, t_field("c", "v"), t_int(1)),
            ))],
        );
        let out = render_func(&f, &f.body, &counter_types());
        assert!(out.contains("-> i64"), "missing return type: {out}");
        assert!(
            out.contains("let result ="),
            "missing return binding: {out}"
        );
        // The trailing bare expression returns the scalar.
        assert!(
            out.lines().any(|l| l.trim() == "result"),
            "missing return expression: {out}"
        );
    }

    // ---- collect_types ----

    #[test]
    fn collect_types_gathers_struct_fields() {
        let module = Module {
            attributes: vec![],
            name: "M".into(),
            items: vec![Item::Func(t_func(
                "f",
                vec![Param {
                    name: "c".into(),
                    ty: Type::Named("Counter".into()),
                    mutability: ParamMutability::Immutable,
                }],
                vec![t_bin(BinOp::Ge, t_field("c", "v"), t_int(0))],
                vec![],
                vec![Stmt::MutateState(vec![t_assign(
                    t_field("c", "v"),
                    AssignOp::Set,
                    t_int(0),
                )])],
            ))],
        };
        let mut types = HashMap::new();
        collect_types(&module, &mut types);
        assert_eq!(types.get("Counter").map(|s| s.len()), Some(1));
        assert!(types.get("Counter").unwrap().iter().any(|(n, _)| n == "v"));
    }

    // ---- go::exported ----

    #[test]
    fn go_exported_capitalises() {
        assert_eq!(go::exported("enqueue"), "Enqueue");
        assert_eq!(go::exported("pending"), "Pending");
        assert_eq!(go::exported("Queue"), "Queue");
    }

    // ---- eject hatch ----

    #[test]
    fn eject_renders_opaque_block_and_guard() {
        let mut f = t_func(
            "withdraw",
            vec![Param {
                name: "b".into(),
                ty: Type::Named("Balance".into()),
                mutability: ParamMutability::Immutable,
            }],
            vec![t_bin(BinOp::Ge, t_field("b", "amount"), t_int(0))],
            vec![t_bin(
                BinOp::Eq,
                t_field("b", "amount"),
                t_bin(BinOp::Sub, t_old(t_field("b", "amount")), t_int(1)),
            )],
            vec![Stmt::MutateState(vec![t_assign(
                t_field("b", "amount"),
                AssignOp::Set,
                t_int(0),
            )])],
        );
        f.attributes.push(Attribute {
            name: "eject".into(),
            args: vec![Arg::Flag("rust".into())],
        });

        let mut types = HashMap::new();
        types.insert(
            "Balance".to_string(),
            vec![("amount".to_string(), Type::int())],
        );
        let out = eject::render_rust_ejected(&f, &f.body, &types);

        // The trusted opaque block and the generated guard are both present.
        assert!(
            out.contains("fn withdraw_impl"),
            "missing opaque block: {out}"
        );
        assert!(out.contains("pub fn withdraw"), "missing guard: {out}");
        // The guard enforces the contracts with runtime assertions.
        assert!(out.contains("assert!("), "missing contract guard: {out}");
        // `old(...)` is captured into a snapshot local in the guard.
        assert!(
            out.contains("__old_b_amount"),
            "missing old snapshot: {out}"
        );
    }

    // ---- generate_program (structural) ----

    #[test]
    fn generate_program_emits_struct_and_invariant_method() {
        // The struct fields come from the function's field accesses (the
        // invariant alone does not populate the struct type).
        let src =
            "module M { invariant Counter { v >= 0 } func f(c: Counter) requires c.v >= 0 { } }";
        let modules = tpt_telos_parser::parse(src).unwrap();
        let rust = generate_program(&modules, &[]);
        assert!(rust.contains("pub struct Counter"));
        assert!(rust.contains("pub fn satisfies_invariants"));
        assert!(rust.contains("pub fn f"));
    }

    #[test]
    fn persistent_storage_emits_serde_derive() {
        let src = "@state(persistent) module M { invariant Counter { v >= 0 } func f(c: Counter) requires c.v >= 0 { } }";
        let modules = tpt_telos_parser::parse(src).unwrap();
        let rust = generate_program(&modules, &[]);
        assert!(
            rust.contains("#[derive(serde::Serialize, serde::Deserialize)]"),
            "expected serde derive for persistent module:\n{rust}"
        );
    }

    #[test]
    fn ephemeral_storage_no_serde_derive() {
        let src =
            "module M { invariant Counter { v >= 0 } func f(c: Counter) requires c.v >= 0 { } }";
        let modules = tpt_telos_parser::parse(src).unwrap();
        let rust = generate_program(&modules, &[]);
        assert!(
            !rust.contains("serde"),
            "ephemeral module should not have serde:\n{rust}"
        );
    }
}
