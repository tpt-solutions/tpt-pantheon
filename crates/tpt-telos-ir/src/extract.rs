//! Extraction of verification problems from the AST.
//!
//! For each `func`, we build:
//! * `premises` -- requires, type constraints, entry invariants, frame
//!   axioms, and `mutate state` assignments (which define post-state
//!   variables).
//! * `conclusions` -- `ensures` clauses and invariants that must hold in the
//!   post-state.
//!
//! State variables are named `"<base>.<field>"` in the pre-state and
//! `"<base>.<field>'"` in the post-state. `old(e)` always resolves to the
//! pre-state name.

use crate::ir::*;
use std::collections::{HashMap, HashSet};
use tpt_telos_parser::ast::*;

/// Per-variable lower and upper bounds derived from premise constraints.
/// `None` means the bound is unknown.
type VarBounds = HashMap<String, (Option<i64>, Option<i64>)>;

type Naming = dyn Fn(&str, &str) -> String;

fn pre_field(base: &str, field: &str) -> String {
    format!("{}.{}", base, field)
}

fn post_field(base: &str, field: &str) -> String {
    format!("{}.{}'", base, field)
}

/// Recursively replace bare `Var` identifiers with `Field { base, var }`.
/// Used to specialise a type-level invariant (e.g. `balance >= 0`) for a
/// particular binding (e.g. `from`).
fn instantiate(expr: &Expr, base: &str) -> Expr {
    match expr {
        Expr::Var(v) => Expr::Field {
            base: base.to_string(),
            field: v.clone(),
        },
        Expr::Field { .. } => expr.clone(),
        Expr::Int(_) | Expr::Old(_) => expr.clone(),
        Expr::Unary { op, expr } => Expr::Unary {
            op: *op,
            expr: Box::new(instantiate(expr, base)),
        },
        Expr::Bin { op, lhs, rhs } => Expr::Bin {
            op: *op,
            lhs: Box::new(instantiate(lhs, base)),
            rhs: Box::new(instantiate(rhs, base)),
        },
        // New expression kinds are cloned as-is (they are not lowered to IR
        // constraints; they appear only in codegen or runtime paths).
        other => other.clone(),
    }
}

fn linearize(
    expr: &Expr,
    field_fn: &Naming,
    var_fn: &dyn Fn(&str) -> String,
) -> Result<Linear, String> {
    match expr {
        Expr::Int(n) => Ok(Linear::constant_only(*n)),
        Expr::Var(name) => Ok(Linear::var(&var_fn(name))),
        Expr::Field { base, field } => Ok(Linear::var(&field_fn(base, field))),
        Expr::Old(e) => linearize(e, &|b, f| pre_field(b, f), var_fn),
        Expr::Unary { op, expr } => match op {
            UnOp::Neg => Ok(linearize(expr, field_fn, var_fn)?.neg()),
        },
        Expr::Bin { op, lhs, rhs } => {
            let l = linearize(lhs, field_fn, var_fn)?;
            let r = linearize(rhs, field_fn, var_fn)?;
            match op {
                BinOp::Add => Ok(l.add(&r)),
                BinOp::Sub => Ok(l.sub(&r)),
                BinOp::Mul => {
                    if let Some(k) = as_int(rhs) {
                        Ok(l.scale(k))
                    } else if let Some(k) = as_int(lhs) {
                        Ok(r.scale(k))
                    } else {
                        Err("non-linear multiplication is not supported in constraints".into())
                    }
                }
                BinOp::Div => {
                    if let Some(k) = as_int(rhs) {
                        if k == 0 {
                            Err("division by zero".into())
                        } else {
                            Ok(scale_exact(&l, k)?)
                        }
                    } else {
                        Err("division must be by a constant in constraints".into())
                    }
                }
                _ => Err("expected arithmetic expression".into()),
            }
        }
        Expr::Index(i) => {
            if let Some(idx) = as_int(&i.index) {
                // Constant index: treat arr[i] as a field access arr.i
                match &*i.receiver {
                    Expr::Field { base, .. } => Ok(Linear::var(&field_fn(base, &idx.to_string()))),
                    Expr::Var(name) => Ok(Linear::var(&var_fn(&format!("{}.{}", name, idx)))),
                    _ => Err("array index receiver must be a field or variable".into()),
                }
            } else {
                Err("non-constant array index in contract is not supported".into())
            }
        }
        // New expression kinds (Call, If, Match, etc.) cannot be lowered to
        // linear arithmetic. They are handled at the codegen level only.
        other => Err(format!(
            "expression `{}` cannot be lowered to linear arithmetic",
            pretty(other)
        )),
    }
}

/// Scan premise constraints to extract per-variable lower and upper bounds.
///
/// Only single-variable constraints with coefficient ±1 are processed; multi-
/// variable constraints are ignored because they don't isolate a single bound.
fn collect_var_bounds(premises: &[Constraint]) -> VarBounds {
    let mut bounds: VarBounds = HashMap::new();
    for Constraint(lin, rel) in premises {
        if lin.terms.len() != 1 {
            continue;
        }
        let (var, coeff) = &lin.terms[0];
        let c = lin.constant;
        // The constraint is `coeff * var + c  rel  0`.
        let (lo_delta, hi_delta): (Option<i64>, Option<i64>) = match (*coeff, rel) {
            // var + c >= 0  →  var >= -c
            (1, Relation::Ge) => (Some(-c), None),
            // var + c > 0   →  var > -c  →  var >= -c + 1 (integers)
            (1, Relation::Gt) => (Some(-c + 1), None),
            // var + c <= 0  →  var <= -c
            (1, Relation::Le) => (None, Some(-c)),
            // var + c < 0   →  var < -c  →  var <= -c - 1
            (1, Relation::Lt) => (None, Some(-c - 1)),
            // -var + c >= 0  →  var <= c
            (-1, Relation::Ge) => (None, Some(c)),
            // -var + c > 0   →  var < c  →  var <= c - 1
            (-1, Relation::Gt) => (None, Some(c - 1)),
            // -var + c <= 0  →  var >= c
            (-1, Relation::Le) => (Some(c), None),
            // -var + c < 0   →  var > c  →  var >= c + 1
            (-1, Relation::Lt) => (Some(c + 1), None),
            // var + c == 0  →  var == -c  (both lo and hi)
            (1, Relation::Eq) => (Some(-c), Some(-c)),
            _ => continue,
        };
        let entry = bounds.entry(var.clone()).or_insert((None, None));
        if let Some(lo) = lo_delta {
            entry.0 = Some(entry.0.map_or(lo, |e: i64| e.max(lo)));
        }
        if let Some(hi) = hi_delta {
            entry.1 = Some(entry.1.map_or(hi, |e: i64| e.min(hi)));
        }
    }
    bounds
}

/// Look up bounds for `var`, falling back to the pre-state version when
/// `var` is a post-state name (ends with `'`). Frame axioms make pre-state
/// bounds valid for unmodified post-state variables.
fn lookup_bounds<'a>(bounds: &'a VarBounds, var: &str) -> Option<&'a (Option<i64>, Option<i64>)> {
    bounds.get(var).or_else(|| {
        if let Some(pre) = var.strip_suffix('\'') {
            bounds.get(pre)
        } else {
            None
        }
    })
}

/// Return the resolved variable name for a simple variable expression, or
/// `None` for complex sub-expressions we cannot name.
fn expr_var_name(
    expr: &Expr,
    field_fn: &Naming,
    var_fn: &dyn Fn(&str) -> String,
) -> Option<String> {
    match expr {
        Expr::Var(name) => Some(var_fn(name)),
        Expr::Field { base, field } => Some(field_fn(base, field)),
        _ => None,
    }
}

/// Like [`linearize`] but attempts interval-arithmetic bounding for nonlinear
/// products when both operands have known bounds in `bounds`. Sets
/// `*approximated = true` when a product was replaced by its interval bound.
fn linearize_bounded(
    expr: &Expr,
    field_fn: &Naming,
    var_fn: &dyn Fn(&str) -> String,
    bounds: &VarBounds,
    approximated: &mut bool,
    prefer_min_corner: bool,
) -> Result<Linear, String> {
    match expr {
        Expr::Int(n) => Ok(Linear::constant_only(*n)),
        Expr::Var(name) => Ok(Linear::var(&var_fn(name))),
        Expr::Field { base, field } => Ok(Linear::var(&field_fn(base, field))),
        Expr::Old(e) => linearize_bounded(
            e,
            &|b, f| pre_field(b, f),
            var_fn,
            bounds,
            approximated,
            prefer_min_corner,
        ),
        Expr::Unary { op, expr } => match op {
            UnOp::Neg => Ok(linearize_bounded(
                expr,
                field_fn,
                var_fn,
                bounds,
                approximated,
                prefer_min_corner,
            )?
            .neg()),
        },
        Expr::Bin { op, lhs, rhs } => {
            let l = linearize_bounded(
                lhs,
                field_fn,
                var_fn,
                bounds,
                approximated,
                prefer_min_corner,
            )?;
            let r = linearize_bounded(
                rhs,
                field_fn,
                var_fn,
                bounds,
                approximated,
                prefer_min_corner,
            )?;
            match op {
                BinOp::Add => Ok(l.add(&r)),
                BinOp::Sub => Ok(l.sub(&r)),
                BinOp::Mul => {
                    if let Some(k) = as_int(rhs) {
                        Ok(l.scale(k))
                    } else if let Some(k) = as_int(lhs) {
                        Ok(r.scale(k))
                    } else {
                        // Try interval bounding: replace a * b with a constant
                        // derived from the product of their bounds. `prefer_min_corner`
                        // selects min vs max so callers can request both and check both
                        // extremes for soundness (see `to_constraints_bounded_dnf`).
                        let a_name = expr_var_name(lhs, field_fn, var_fn);
                        let b_name = expr_var_name(rhs, field_fn, var_fn);
                        if let (Some(ref a), Some(ref b)) = (a_name, b_name) {
                            let ab = lookup_bounds(bounds, a);
                            let bb = lookup_bounds(bounds, b);
                            if let (
                                Some((Some(a_lo), Some(a_hi))),
                                Some((Some(b_lo), Some(b_hi))),
                            ) = (ab, bb)
                            {
                                let corners = [
                                    a_lo.saturating_mul(*b_lo),
                                    a_lo.saturating_mul(*b_hi),
                                    a_hi.saturating_mul(*b_lo),
                                    a_hi.saturating_mul(*b_hi),
                                ];
                                let product_bound = if prefer_min_corner {
                                    *corners.iter().min().unwrap()
                                } else {
                                    *corners.iter().max().unwrap()
                                };
                                *approximated = true;
                                return Ok(Linear::constant_only(product_bound));
                            }
                        }
                        Err(format!(
                            "non-linear multiplication ({} * {}) cannot be verified; \
                             add `requires` bounds on both variables to enable \
                             interval-bounding approximation",
                            pretty(lhs),
                            pretty(rhs)
                        ))
                    }
                }
                BinOp::Div => {
                    if let Some(k) = as_int(rhs) {
                        if k == 0 {
                            Err("division by zero".into())
                        } else {
                            Ok(scale_exact(&l, k)?)
                        }
                    } else {
                        Err("division must be by a constant in constraints".into())
                    }
                }
                _ => Err("expected arithmetic expression".into()),
            }
        }
        Expr::Index(i) => {
            if let Some(idx) = as_int(&i.index) {
                match &*i.receiver {
                    Expr::Field { base, .. } => Ok(Linear::var(&field_fn(base, &idx.to_string()))),
                    Expr::Var(name) => Ok(Linear::var(&var_fn(&format!("{}.{}", name, idx)))),
                    _ => Err("array index receiver must be a field or variable".into()),
                }
            } else {
                Err("non-constant array index in contract is not supported".into())
            }
        }
        // New expression kinds (Call, If, Match, etc.) cannot be lowered to
        // linear arithmetic. They are handled at the codegen level only.
        other => Err(format!(
            "expression `{}` cannot be lowered to linear arithmetic",
            pretty(other)
        )),
    }
}

/// Like [`to_constraints`] but uses [`linearize_bounded`] to handle nonlinear
/// products via interval-arithmetic bounding. Sets `*approximated = true` when
/// any product was replaced by an interval-bounded constant.
fn to_constraints_bounded(
    expr: &Expr,
    field_fn: &Naming,
    var_fn: &dyn Fn(&str) -> String,
    bounds: &VarBounds,
    approximated: &mut bool,
) -> Result<Vec<Constraint>, String> {
    match expr {
        Expr::Bin { op, lhs, rhs } if *op == BinOp::And => {
            let mut out = to_constraints_bounded(lhs, field_fn, var_fn, bounds, approximated)?;
            out.extend(to_constraints_bounded(
                rhs,
                field_fn,
                var_fn,
                bounds,
                approximated,
            )?);
            Ok(out)
        }
        Expr::Bin { op, lhs, rhs } if relation_of(*op).is_some() => {
            let rel = relation_of(*op).unwrap();
            let l = linearize_bounded(lhs, field_fn, var_fn, bounds, approximated, false)?;
            let r = linearize_bounded(rhs, field_fn, var_fn, bounds, approximated, false)?;
            let diff = l.sub(&r);
            Ok(vec![Constraint(diff, rel)])
        }
        _ => Err("expected a boolean constraint (comparison or `&&`)".into()),
    }
}

/// Like [`to_constraints_bounded`] but produces Disjunctive Normal Form: a list
/// of branches, each a conjunction of constraints. `||` creates two branches;
/// `&&` flattens within each branch.
fn to_constraints_bounded_dnf(
    expr: &Expr,
    field_fn: &Naming,
    var_fn: &dyn Fn(&str) -> String,
    bounds: &VarBounds,
    approximated: &mut bool,
) -> Result<Vec<Vec<Constraint>>, String> {
    match expr {
        Expr::Bin { op, lhs, rhs } if *op == BinOp::Or => {
            let left = to_constraints_bounded_dnf(lhs, field_fn, var_fn, bounds, approximated)?;
            let right = to_constraints_bounded_dnf(rhs, field_fn, var_fn, bounds, approximated)?;
            let mut out = left;
            out.extend(right);
            Ok(out)
        }
        Expr::Bin { op, lhs, rhs } if *op == BinOp::And => {
            let left = to_constraints_bounded_dnf(lhs, field_fn, var_fn, bounds, approximated)?;
            let right = to_constraints_bounded_dnf(rhs, field_fn, var_fn, bounds, approximated)?;
            Ok(combine_dnf(&left, &right))
        }
        Expr::Bin { op, lhs, rhs } if relation_of(*op).is_some() => {
            let rel = relation_of(*op).unwrap();
            // Compute the max-corner bound first.
            let mut approx_max = false;
            let l_max = linearize_bounded(lhs, field_fn, var_fn, bounds, &mut approx_max, false)?;
            let r_max = linearize_bounded(rhs, field_fn, var_fn, bounds, &mut approx_max, false)?;
            let diff_max = l_max.sub(&r_max);
            if approx_max {
                // When interval bounding was used, also compute the min-corner bound
                // and include both constraints in the same conjunction. This is sound
                // regardless of the relation direction: for `>=`, the binding check is
                // the min-corner; for `<=`, the binding check is the max-corner.
                let mut approx_min = false;
                let l_min =
                    linearize_bounded(lhs, field_fn, var_fn, bounds, &mut approx_min, true)?;
                let r_min =
                    linearize_bounded(rhs, field_fn, var_fn, bounds, &mut approx_min, true)?;
                let diff_min = l_min.sub(&r_min);
                *approximated = true;
                Ok(vec![vec![
                    Constraint(diff_max, rel.clone()),
                    Constraint(diff_min, rel),
                ]])
            } else {
                Ok(vec![vec![Constraint(diff_max, rel)]])
            }
        }
        _ => Err("expected a boolean constraint (comparison, `&&`, or `||`)".into()),
    }
}

fn as_int(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Int(n) => Some(*n),
        _ => None,
    }
}

fn scale_exact(l: &Linear, k: i64) -> Result<Linear, String> {
    if l.constant % k != 0 {
        return Err("non-integer division in constraint".into());
    }
    let mut terms = Vec::new();
    for (v, c) in &l.terms {
        if c % k != 0 {
            return Err("non-integer division in constraint".into());
        }
        terms.push((v.clone(), c / k));
    }
    Ok(Linear {
        terms,
        constant: l.constant / k,
    })
}

fn relation_of(op: BinOp) -> Option<Relation> {
    match op {
        BinOp::Eq => Some(Relation::Eq),
        BinOp::Ne => Some(Relation::Ne),
        BinOp::Lt => Some(Relation::Lt),
        BinOp::Le => Some(Relation::Le),
        BinOp::Gt => Some(Relation::Gt),
        BinOp::Ge => Some(Relation::Ge),
        _ => None,
    }
}

/// Lower a boolean expression into one or more linear constraints.
/// `&&` is flattened; `||` is rejected (handled at the conclusion level later
/// if ever needed).
fn to_constraints(
    expr: &Expr,
    field_fn: &Naming,
    var_fn: &dyn Fn(&str) -> String,
) -> Result<Vec<Constraint>, String> {
    let dnf = to_constraints_dnf(expr, field_fn, var_fn)?;
    if dnf.len() == 1 {
        Ok(dnf.into_iter().next().unwrap())
    } else {
        Err("disjunction (`||`) is not allowed in this position".into())
    }
}

/// Lower a boolean expression into Disjunctive Normal Form (DNF): a list of
/// conjunction branches. Each inner `Vec<Constraint>` is one branch (a
/// conjunction of constraints). `||` creates two branches; `&&` flattens
/// within each branch.
fn to_constraints_dnf(
    expr: &Expr,
    field_fn: &Naming,
    var_fn: &dyn Fn(&str) -> String,
) -> Result<Vec<Vec<Constraint>>, String> {
    match expr {
        Expr::Bin { op, lhs, rhs } if *op == BinOp::Or => {
            let left = to_constraints_dnf(lhs, field_fn, var_fn)?;
            let right = to_constraints_dnf(rhs, field_fn, var_fn)?;
            let mut out = left;
            out.extend(right);
            Ok(out)
        }
        Expr::Bin { op, lhs, rhs } if *op == BinOp::And => {
            let left = to_constraints_dnf(lhs, field_fn, var_fn)?;
            let right = to_constraints_dnf(rhs, field_fn, var_fn)?;
            Ok(combine_dnf(&left, &right))
        }
        Expr::Bin { op, lhs, rhs } if relation_of(*op).is_some() => {
            let rel = relation_of(*op).unwrap();
            let l = linearize(lhs, field_fn, var_fn)?;
            let r = linearize(rhs, field_fn, var_fn)?;
            let diff = l.sub(&r);
            Ok(vec![vec![Constraint(diff, rel)]])
        }
        Expr::If(i) => {
            // if cond { a } else { b }  =>  (cond && a) || (!cond && b)
            let cond_branch = to_constraints_dnf(&i.condition, field_fn, var_fn)?;
            let then_branch = to_constraints_dnf(&i.then_expr, field_fn, var_fn)?;
            let else_branch = to_constraints_dnf(&i.else_expr, field_fn, var_fn)?;
            let neg_cond = negate_dnf(&cond_branch);
            let mut out = combine_dnf(&cond_branch, &then_branch);
            out.extend(combine_dnf(&neg_cond, &else_branch));
            Ok(out)
        }
        Expr::Match(m) => {
            // match scrutinee { pattern => body, ... }
            // Each arm: (scrutinee == pattern_value) && body.
            // Only Literal patterns can be expressed as linear premises;
            // Wildcard/Var/Constructor arms fall through without a premise.
            let scrutinee_lin = linearize(&m.scrutinee, field_fn, var_fn);
            let mut out = Vec::new();
            for arm in &m.arms {
                let arm_branches = to_constraints_dnf(&arm.expr, field_fn, var_fn)?;
                match &arm.pattern {
                    tpt_telos_parser::ast::Pattern::Literal(n) => {
                        if let Ok(ref s) = scrutinee_lin {
                            let diff = s.sub(&Linear::constant_only(*n));
                            let premise_dnf = vec![vec![Constraint(diff, Relation::Eq)]];
                            out.extend(combine_dnf(&premise_dnf, &arm_branches));
                        } else {
                            out.extend(arm_branches);
                        }
                    }
                    _ => out.extend(arm_branches),
                }
            }
            if out.is_empty() {
                Ok(vec![vec![]])
            } else {
                Ok(out)
            }
        }
        Expr::Forall(f) => {
            // Bounded forall: unroll when domain is a constant range.
            if let Some(domain) = &f.domain {
                match &**domain {
                    Expr::Range { lo, hi } => {
                        let lo_val = eval_const_expr(lo).ok_or_else(|| {
                            format!(
                                "forall range lower bound `{}` must be a constant",
                                pretty(lo)
                            )
                        })?;
                        let hi_val = eval_const_expr(hi).ok_or_else(|| {
                            format!(
                                "forall range upper bound `{}` must be a constant",
                                pretty(hi)
                            )
                        })?;
                        if lo_val >= hi_val {
                            return Ok(vec![vec![]]); // empty range: vacuously true
                        }
                        // Unroll: conjunction of body instantiated for each i in lo..hi
                        let mut combined: Vec<Vec<Constraint>> = vec![vec![]];
                        for i in lo_val..hi_val {
                            let inst = instantiate_forall(f, i);
                            let branch = to_constraints_dnf(&inst, field_fn, var_fn)?;
                            combined = combine_dnf(&combined, &branch);
                        }
                        Ok(combined)
                    }
                    _ => Err(format!(
                        "forall with non-range domain `{}` is not supported in contracts",
                        pretty(domain)
                    )),
                }
            } else {
                Err("forall without domain is not supported in contracts".into())
            }
        }
        Expr::Aggregate(a) => {
            // Aggregates over ranges: sum(i in lo..hi) { body }, etc.
            // For now, only support single-argument aggregates over a range.
            if a.args.len() == 1 {
                if let Expr::Forall(f) = &a.args[0] {
                    if let Some(domain) = &f.domain {
                        if let Expr::Range { lo, hi } = &**domain {
                            let lo_val = eval_const_expr(lo).ok_or_else(|| {
                                "aggregate range lower bound must be a constant".to_string()
                            })?;
                            let hi_val = eval_const_expr(hi).ok_or_else(|| {
                                "aggregate range upper bound must be a constant".to_string()
                            })?;
                            if lo_val >= hi_val {
                                return Ok(vec![vec![]]); // empty range
                            }
                            // Unroll aggregate to linear expression.
                            let mut result = Linear::constant_only(0);
                            let mut first = true;
                            for i in lo_val..hi_val {
                                let inst = instantiate_forall(f, i);
                                let val = linearize(&inst, field_fn, var_fn)?;
                                match a.op {
                                    AggregateOp::Sum => {
                                        result = if first { val } else { result.add(&val) };
                                    }
                                    AggregateOp::Min | AggregateOp::Max => {
                                        // For min/max, we can't represent this as a single
                                        // linear constraint. Fall back to error.
                                        return Err(format!(
                                            "{} over ranges is not yet supported in contracts",
                                            a.op.op_name()
                                        ));
                                    }
                                    AggregateOp::Count => {
                                        result = if first {
                                            Linear::constant_only(1)
                                        } else {
                                            result.add(&Linear::constant_only(1))
                                        };
                                    }
                                }
                                first = false;
                            }
                            // The aggregate result is used as a value in a comparison.
                            // We need to return it as a constraint against 0.
                            // Actually, this needs to be integrated into the comparison.
                            // For now, return the result as a comparison with itself == result.
                            // This is a simplification — the aggregate should be used in a comparison.
                            return Err("aggregate result must be used in a comparison (e.g. sum(...) == value)".into());
                        }
                    }
                }
            }
            Err("aggregate expressions are not fully supported in contracts".into())
        }
        _ => Err("expected a boolean constraint (comparison, `&&`, or `||`)".into()),
    }
}

/// Evaluate an expression to a constant integer. Returns `None` for
/// non-constant expressions.
fn eval_const_expr(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Int(n) => Some(*n),
        Expr::Unary {
            op: UnOp::Neg,
            expr,
        } => eval_const_expr(expr).map(|v| -v),
        Expr::Bin { op, lhs, rhs } => {
            let l = eval_const_expr(lhs)?;
            let r = eval_const_expr(rhs)?;
            match op {
                BinOp::Add => Some(l + r),
                BinOp::Sub => Some(l - r),
                BinOp::Mul => Some(l * r),
                BinOp::Div => {
                    if r == 0 {
                        None
                    } else {
                        Some(l / r)
                    }
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Instantiate a forall body by replacing the quantified variable with a
/// concrete integer value.
fn instantiate_forall(f: &ForallExpr, value: i64) -> Expr {
    substitute_var(&f.body, &f.var, &Expr::Int(value))
}

/// Replace all occurrences of `var` in `expr` with `replacement`.
fn substitute_var(expr: &Expr, var: &str, replacement: &Expr) -> Expr {
    match expr {
        Expr::Var(v) if v == var => replacement.clone(),
        Expr::Var(_) | Expr::Int(_) | Expr::Field { .. } => expr.clone(),
        Expr::Old(e) => Expr::Old(Box::new(substitute_var(e, var, replacement))),
        Expr::Unary { op, expr } => Expr::Unary {
            op: *op,
            expr: Box::new(substitute_var(expr, var, replacement)),
        },
        Expr::Bin { op, lhs, rhs } => Expr::Bin {
            op: *op,
            lhs: Box::new(substitute_var(lhs, var, replacement)),
            rhs: Box::new(substitute_var(rhs, var, replacement)),
        },
        Expr::Forall(f) => Expr::Forall(ForallExpr {
            var: f.var.clone(),
            var_ty: f.var_ty.clone(),
            domain: f
                .domain
                .as_ref()
                .map(|d| Box::new(substitute_var(d, var, replacement))),
            body: Box::new(substitute_var(&f.body, var, replacement)),
        }),
        Expr::Aggregate(a) => Expr::Aggregate(AggregateExpr {
            op: a.op,
            args: a
                .args
                .iter()
                .map(|e| substitute_var(e, var, replacement))
                .collect(),
        }),
        Expr::Range { lo, hi } => Expr::Range {
            lo: Box::new(substitute_var(lo, var, replacement)),
            hi: Box::new(substitute_var(hi, var, replacement)),
        },
        other => other.clone(),
    }
}

/// Cartesian product of two DNF forms: every branch in `a` combined with every
/// branch in `b`.
fn combine_dnf(a: &[Vec<Constraint>], b: &[Vec<Constraint>]) -> Vec<Vec<Constraint>> {
    let mut out = Vec::new();
    for a_branch in a {
        for b_branch in b {
            let mut combined = a_branch.clone();
            combined.extend(b_branch.iter().cloned());
            out.push(combined);
        }
    }
    out
}

/// Negate a DNF form using De Morgan's laws:
///   !(A || B || ...) = !A && !B && ...
/// where negating one conjunction [c1, c2, ...] yields [!c1] || [!c2] || ...
/// The result is itself in DNF form (a list of conjunction branches).
fn negate_dnf(dnf: &[Vec<Constraint>]) -> Vec<Vec<Constraint>> {
    // Start with `true` (single empty branch, represents "no constraints").
    let mut result: Vec<Vec<Constraint>> = vec![vec![]];
    for conj_branch in dnf {
        if conj_branch.is_empty() {
            // An empty conjunction is `true`; !(true) = false → empty DNF.
            return vec![];
        }
        // !(c1 && c2 && ...) = !c1 || !c2 || ...
        // Each negated constraint becomes its own single-constraint branch.
        let neg_conj: Vec<Vec<Constraint>> = conj_branch
            .iter()
            .map(|c| vec![Constraint(c.0.clone(), negate_relation(&c.1))])
            .collect();
        // AND the negated conjunction into the accumulated result.
        result = combine_dnf(&result, &neg_conj);
    }
    result
}

/// Negate a single relation as one constraint (`x R y` -> `x (not R) y`).
/// Unlike `solver::negate` (which splits `Eq`'s negation into `Lt || Gt`
/// because the solver needs concrete leaf relations to check unsatisfiability
/// of each disjunct), this uses `Relation::Ne` directly for `Eq`'s negation:
/// the Fourier-Motzkin solver treats `Ne` premises as a no-op (sound but
/// uninformative) rather than a relation it needs to case-split on, so one
/// branch here avoids needlessly doubling the number of body-mutation
/// branches for a plain `if x == y`/`if x != y` condition.
fn negate_relation(rel: &Relation) -> Relation {
    match rel {
        Relation::Eq => Relation::Ne,
        Relation::Ne => Relation::Eq,
        Relation::Le => Relation::Gt,
        Relation::Lt => Relation::Ge,
        Relation::Ge => Relation::Lt,
        Relation::Gt => Relation::Le,
    }
}

/// One branch of a function body's mutation analysis: the extra premises
/// that characterize when this branch is taken (from `if`/`else` guards),
/// the mutation constraints it applies, and the set of fields it assigns.
struct BodyBranch {
    extra_premises: Vec<Constraint>,
    mutations: Vec<Constraint>,
    assigned: HashSet<(String, String)>,
}

/// Recursively collect every field referenced by a function body's
/// statements (assignment targets and values), descending into `if`/`match`
/// arms so frame axioms account for fields touched only in nested branches.
fn collect_body_referenced_fields(body: &[Stmt], out: &mut HashSet<(String, String)>) {
    for stmt in body {
        match stmt {
            Stmt::MutateState(assigns) => {
                for a in assigns {
                    collect_fields(&a.value, out);
                    if let Expr::Field { base, field } = &a.target {
                        out.insert((base.clone(), field.clone()));
                    }
                }
            }
            Stmt::Assign(a) => {
                collect_fields(&a.value, out);
                if let Expr::Field { base, field } = &a.target {
                    out.insert((base.clone(), field.clone()));
                }
            }
            Stmt::If(if_stmt) => {
                collect_body_referenced_fields(&if_stmt.then_body, out);
                if let Some(else_body) = &if_stmt.else_body {
                    collect_body_referenced_fields(else_body, out);
                }
            }
            Stmt::Match(match_stmt) => {
                for arm in &match_stmt.arms {
                    collect_body_referenced_fields(&arm.body, out);
                }
            }
            Stmt::Let(_) | Stmt::Return(_) => {}
        }
    }
}

/// Analyze a function body's statements into mutation branches. A bare
/// `mutate state`/assignment extends every open branch; a top-level `if`
/// with a single-comparison condition splits into a "then" branch (guarded
/// by the condition) and an "else" branch (guarded by its negation), each
/// recursively analyzed. `match` arms that would mutate state are rejected
/// with a specific error rather than silently dropped, since that shape
/// isn't supported yet.
fn process_body_stmts(
    stmts: &[Stmt],
    branches: Vec<BodyBranch>,
    field_fn: &Naming,
    var_fn: &dyn Fn(&str) -> String,
) -> Result<Vec<BodyBranch>, String> {
    let mut branches = branches;
    for stmt in stmts {
        match stmt {
            Stmt::MutateState(assigns) => {
                for a in assigns {
                    let c = assign_constraint(a, field_fn, var_fn)?;
                    let field = match &a.target {
                        Expr::Field { base, field } => Some((base.clone(), field.clone())),
                        _ => None,
                    };
                    for b in &mut branches {
                        b.mutations.push(c.clone());
                        if let Some(f) = &field {
                            b.assigned.insert(f.clone());
                        }
                    }
                }
            }
            Stmt::Assign(a) => {
                let c = assign_constraint(a, field_fn, var_fn)?;
                let field = match &a.target {
                    Expr::Field { base, field } => Some((base.clone(), field.clone())),
                    _ => None,
                };
                for b in &mut branches {
                    b.mutations.push(c.clone());
                    if let Some(f) = &field {
                        b.assigned.insert(f.clone());
                    }
                }
            }
            Stmt::If(if_stmt) => {
                let (diff, rel) = match &if_stmt.condition {
                    Expr::Bin { op, lhs, rhs } if relation_of(*op).is_some() => {
                        let rel = relation_of(*op).unwrap();
                        let l = linearize(lhs, field_fn, var_fn)?;
                        let r = linearize(rhs, field_fn, var_fn)?;
                        (l.sub(&r), rel)
                    }
                    other => {
                        return Err(format!(
                            "`if` inside a function body must have a single comparison \
                             condition (found `{}`); compound (`&&`/`||`) conditions are not \
                             yet supported here",
                            pretty(other)
                        ));
                    }
                };
                let not_rel = negate_relation(&rel);
                let cond_c = Constraint(diff.clone(), rel);
                let not_cond_c = Constraint(diff.clone(), not_rel);

                let mut then_branches: Vec<BodyBranch> = branches
                    .iter()
                    .map(|b| BodyBranch {
                        extra_premises: {
                            let mut p = b.extra_premises.clone();
                            p.push(cond_c.clone());
                            p
                        },
                        mutations: b.mutations.clone(),
                        assigned: b.assigned.clone(),
                    })
                    .collect();
                then_branches =
                    process_body_stmts(&if_stmt.then_body, then_branches, field_fn, var_fn)?;

                let else_branches: Vec<BodyBranch> = branches
                    .iter()
                    .map(|b| BodyBranch {
                        extra_premises: {
                            let mut p = b.extra_premises.clone();
                            p.push(not_cond_c.clone());
                            p
                        },
                        mutations: b.mutations.clone(),
                        assigned: b.assigned.clone(),
                    })
                    .collect();
                let else_branches = process_body_stmts(
                    if_stmt.else_body.as_deref().unwrap_or(&[]),
                    else_branches,
                    field_fn,
                    var_fn,
                )?;

                then_branches.extend(else_branches);
                branches = then_branches;
            }
            Stmt::Match(match_stmt) => {
                let mutates = match_stmt.arms.iter().any(|arm| {
                    arm.body.iter().any(|s| {
                        matches!(
                            s,
                            Stmt::MutateState(_) | Stmt::Assign(_) | Stmt::If(_) | Stmt::Match(_)
                        )
                    })
                });
                if mutates {
                    return Err(format!(
                        "`match {}` has a state-mutating arm; `match` in a function body is \
                         not yet supported when any arm mutates state",
                        pretty(&match_stmt.scrutinee)
                    ));
                }
            }
            Stmt::Let(_) | Stmt::Return(_) => {}
        }
    }
    Ok(branches)
}

fn assign_constraint(
    a: &Assign,
    field_fn: &Naming,
    var_fn: &dyn Fn(&str) -> String,
) -> Result<Constraint, String> {
    match &a.target {
        Expr::Field { base, field } => {
            let post = Linear::var(&post_field(base, field));
            let pre = Linear::var(&pre_field(base, field));
            let value = linearize(&a.value, field_fn, var_fn)?;
            let (terms, rel) = match a.op {
                AssignOp::Set => (post.sub(&value), Relation::Eq),
                AssignOp::Add => (post.sub(&pre).sub(&value), Relation::Eq),
                AssignOp::Sub => (post.sub(&pre).add(&value), Relation::Eq),
            };
            Ok(Constraint(terms, rel))
        }
        // Bare local variables (e.g. a scalar `ensures out == ...` output bound
        // by `out = ...;`) have no pre/post-state distinction in this IR — they
        // resolve through the same `var_fn` identity naming used for `ensures`
        // conclusions, so a plain `=` just binds that symbol directly.
        Expr::Var(name) if a.op == AssignOp::Set => {
            let target = Linear::var(&var_fn(name));
            let value = linearize(&a.value, field_fn, var_fn)?;
            Ok(Constraint(target.sub(&value), Relation::Eq))
        }
        Expr::Var(name) => Err(format!(
            "compound assignment ('+='/'-=') to local variable '{}' is not supported; use '='",
            name
        )),
        other => Err(format!(
            "assignment target must be a field or local variable, found {:?}",
            other
        )),
    }
}

/// Collect every `Field { base, field }` referenced in an expression.
fn collect_fields(expr: &Expr, out: &mut HashSet<(String, String)>) {
    match expr {
        Expr::Field { base, field } => {
            out.insert((base.clone(), field.clone()));
        }
        Expr::Old(e) => collect_fields(e, out),
        Expr::Unary { expr, .. } => collect_fields(expr, out),
        Expr::Bin { lhs, rhs, .. } => {
            collect_fields(lhs, out);
            collect_fields(rhs, out);
        }
        Expr::Index(i) => {
            collect_fields(&i.receiver, out);
            collect_fields(&i.index, out);
        }
        Expr::Call(c) => {
            for a in &c.args {
                collect_fields(a, out);
            }
        }
        Expr::MethodCall(m) => {
            collect_fields(&m.receiver, out);
            for a in &m.args {
                collect_fields(a, out);
            }
        }
        Expr::If(i) => {
            collect_fields(&i.condition, out);
            collect_fields(&i.then_expr, out);
            collect_fields(&i.else_expr, out);
        }
        Expr::Match(m) => {
            collect_fields(&m.scrutinee, out);
            for a in &m.arms {
                collect_fields(&a.expr, out);
            }
        }
        _ => {}
    }
}

fn pretty(expr: &Expr) -> String {
    match expr {
        Expr::Int(n) => n.to_string(),
        Expr::Var(v) => v.clone(),
        Expr::Field { base, field } => format!("{}.{}", base, field),
        Expr::Old(e) => format!("old({})", pretty(e)),
        Expr::Unary { op, expr } => match op {
            UnOp::Neg => format!("-{}", pretty(expr)),
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
            format!("{} {} {}", pretty(lhs), s, pretty(rhs))
        }
        Expr::Call(c) => {
            let args: Vec<_> = c.args.iter().map(pretty).collect();
            format!("{}({})", c.func, args.join(", "))
        }
        Expr::MethodCall(m) => {
            let args: Vec<_> = m.args.iter().map(pretty).collect();
            format!("{}.{}({})", pretty(&m.receiver), m.method, args.join(", "))
        }
        Expr::Index(i) => format!("{}[{}]", pretty(&i.receiver), pretty(&i.index)),
        Expr::If(i) => format!(
            "if {} {{ {} }} else {{ {} }}",
            pretty(&i.condition),
            pretty(&i.then_expr),
            pretty(&i.else_expr)
        ),
        Expr::Match(m) => {
            let arms: Vec<_> = m
                .arms
                .iter()
                .map(|a| format!("... => {}", pretty(&a.expr)))
                .collect();
            format!("match {} {{ {} }}", pretty(&m.scrutinee), arms.join(", "))
        }
        Expr::Try(e) => format!("{}?", pretty(e)),
        Expr::Forall(f) => format!(
            "forall {}: {} {{ {} }}",
            f.var,
            f.var_ty.name(),
            pretty(&f.body)
        ),
        Expr::Aggregate(a) => {
            let args: Vec<_> = a.args.iter().map(pretty).collect();
            let op = match a.op {
                AggregateOp::Sum => "sum",
                AggregateOp::Min => "min",
                AggregateOp::Max => "max",
                AggregateOp::Count => "count",
            };
            format!("{}({})", op, args.join(", "))
        }
        Expr::Range { lo, hi } => format!("{}..{}", pretty(lo), pretty(hi)),
    }
}

/// Extract all verification problems from a parsed program.
///
/// # Examples
///
/// ```
/// use tpt_telos_parser::parse;
/// use tpt_telos_ir::extract;
///
/// let src = r#"
///     module Bank {
///         invariant Wallet { balance >= 0 }
///
///         func deposit(w: Wallet, amount: PositiveInt)
///             requires amount > 0
///             ensures w.balance == old(w.balance) + amount
///         { mutate state { w.balance += amount } }
///     }
/// "#;
///
/// let modules = parse(src).unwrap();
/// let problems = extract(&modules).unwrap();
///
/// assert_eq!(problems.len(), 1);
/// assert_eq!(problems[0].func_name, "deposit");
/// // Premises include the requires clause and the entry invariant.
/// assert!(!problems[0].premises.is_empty());
/// // Conclusions include the ensures clause and the exit invariant.
/// assert!(!problems[0].conclusions.is_empty());
/// ```
pub fn extract(modules: &[Module]) -> Result<Vec<VerificationProblem>, String> {
    let mut invariants: HashMap<String, &Invariant> = HashMap::new();
    // Fields declared by each invariant-bearing type, derived from the bare
    // variable names used in the invariant's constraints (e.g. `Wallet {
    // balance >= 0 }` => field `balance`).
    let mut invariant_fields: HashMap<String, HashSet<String>> = HashMap::new();
    for m in modules {
        for item in &m.items {
            if let Item::Invariant(inv) = item {
                invariants.insert(inv.name.clone(), inv);
                let entry = invariant_fields.entry(inv.name.clone()).or_default();
                for c in &inv.constraints {
                    collect_vars(c, entry);
                }
            }
        }
    }

    // Build a map of function names to their specs for modular verification.
    let mut func_specs: HashMap<String, &Func> = HashMap::new();
    for m in modules {
        for item in &m.items {
            if let Item::Func(f) = item {
                func_specs.insert(f.name.clone(), f);
            }
        }
    }

    let mut problems = Vec::new();
    for m in modules {
        for item in &m.items {
            if let Item::Func(f) = item {
                problems.extend(build_problems(
                    f,
                    &invariants,
                    &invariant_fields,
                    &func_specs,
                )?);
            }
        }
    }
    Ok(problems)
}

/// Scan an expression for `Call` nodes and add the callee's ensures as
/// premises. Uses `visited_calls` to avoid infinite recursion on recursive
/// contract references.
fn collect_call_ensures(
    expr: &Expr,
    func_specs: &HashMap<String, &Func>,
    premises: &mut Vec<Constraint>,
    visited: &mut HashSet<String>,
    field_fn: &Naming,
    var_fn: &dyn Fn(&str) -> String,
) -> Result<(), String> {
    match expr {
        Expr::Call(c) => {
            if let Some(callee) = func_specs.get(&c.func) {
                if !visited.contains(&c.func) {
                    visited.insert(callee.name.clone());
                    // Substitute callee parameters with actual arguments.
                    for ensures in &callee.ensures {
                        let mut substituted = ensures.clone();
                        for (param, arg) in callee.params.iter().zip(&c.args) {
                            substituted = substitute_var(&substituted, &param.name, arg);
                        }
                        // Convert to constraints and add as premises.
                        let constraints = to_constraints(&substituted, field_fn, var_fn)?;
                        premises.extend(constraints);
                    }
                }
                // Recurse into arguments.
                for a in &c.args {
                    collect_call_ensures(a, func_specs, premises, visited, field_fn, var_fn)?;
                }
            }
        }
        Expr::MethodCall(m) => {
            collect_call_ensures(&m.receiver, func_specs, premises, visited, field_fn, var_fn)?;
            for a in &m.args {
                collect_call_ensures(a, func_specs, premises, visited, field_fn, var_fn)?;
            }
        }
        Expr::Bin { lhs, rhs, .. } => {
            collect_call_ensures(lhs, func_specs, premises, visited, field_fn, var_fn)?;
            collect_call_ensures(rhs, func_specs, premises, visited, field_fn, var_fn)?;
        }
        Expr::Unary { expr, .. } => {
            collect_call_ensures(expr, func_specs, premises, visited, field_fn, var_fn)?;
        }
        Expr::Old(e) => {
            collect_call_ensures(e, func_specs, premises, visited, field_fn, var_fn)?;
        }
        Expr::If(i) => {
            collect_call_ensures(
                &i.condition,
                func_specs,
                premises,
                visited,
                field_fn,
                var_fn,
            )?;
            collect_call_ensures(
                &i.then_expr,
                func_specs,
                premises,
                visited,
                field_fn,
                var_fn,
            )?;
            collect_call_ensures(
                &i.else_expr,
                func_specs,
                premises,
                visited,
                field_fn,
                var_fn,
            )?;
        }
        Expr::Forall(f) => {
            collect_call_ensures(&f.body, func_specs, premises, visited, field_fn, var_fn)?;
        }
        _ => {}
    }
    Ok(())
}

/// Collect bare variable names referenced in an expression (used to discover an
/// invariant-bearing type's fields).
fn collect_vars(expr: &Expr, out: &mut HashSet<String>) {
    match expr {
        Expr::Var(v) => {
            out.insert(v.clone());
        }
        Expr::Old(e) => collect_vars(e, out),
        Expr::Unary { expr, .. } => collect_vars(expr, out),
        Expr::Bin { lhs, rhs, .. } => {
            collect_vars(lhs, out);
            collect_vars(rhs, out);
        }
        Expr::Index(i) => {
            collect_vars(&i.receiver, out);
            collect_vars(&i.index, out);
        }
        Expr::Call(c) => {
            for a in &c.args {
                collect_vars(a, out);
            }
        }
        Expr::MethodCall(m) => {
            collect_vars(&m.receiver, out);
            for a in &m.args {
                collect_vars(a, out);
            }
        }
        Expr::If(i) => {
            collect_vars(&i.condition, out);
            collect_vars(&i.then_expr, out);
            collect_vars(&i.else_expr, out);
        }
        Expr::Match(m) => {
            collect_vars(&m.scrutinee, out);
            for a in &m.arms {
                collect_vars(&a.expr, out);
            }
        }
        _ => {}
    }
}

fn build_problems(
    func: &Func,
    invariants: &HashMap<String, &Invariant>,
    invariant_fields: &HashMap<String, HashSet<String>>,
    func_specs: &HashMap<String, &Func>,
) -> Result<Vec<VerificationProblem>, String> {
    let var_fn = |name: &str| name.to_string();
    let pre_fn = |b: &str, f: &str| pre_field(b, f);
    let post_fn = |b: &str, f: &str| post_field(b, f);

    // Collect requires clauses as DNF branches.
    // Each requires expression produces one or more branches; we combine them
    // via Cartesian product so `requires a || b` and `requires c` yields two
    // branches: {a, c} and {b, c}.
    let mut premise_dnf: Vec<Vec<Constraint>> = vec![vec![]];
    for r in &func.requires {
        let branches = to_constraints_dnf(r, &pre_fn, &var_fn)?;
        premise_dnf = combine_dnf(&premise_dnf, &branches);
    }

    // Modular verification: for each call in requires/ensures, add the
    // callee's ensures as additional premises. This allows the verifier to
    // use the callee's postconditions when checking the caller's contracts.
    let mut call_ensures_premises: Vec<Constraint> = Vec::new();
    let mut visited_calls: HashSet<String> = HashSet::new();
    for r in &func.requires {
        collect_call_ensures(
            r,
            func_specs,
            &mut call_ensures_premises,
            &mut visited_calls,
            &pre_fn,
            &var_fn,
        )?;
    }
    for e in &func.ensures {
        collect_call_ensures(
            e,
            func_specs,
            &mut call_ensures_premises,
            &mut visited_calls,
            &pre_fn,
            &var_fn,
        )?;
    }
    for branch in &mut premise_dnf {
        branch.extend(call_ensures_premises.iter().cloned());
    }

    // type constraints (e.g. PositiveInt => x >= 1) — these are always
    // conjunctions (no disjunction), so they extend every branch.
    let mut type_constraints: Vec<Constraint> = Vec::new();
    for p in &func.params {
        if p.ty.name().starts_with("Positive") {
            let ge = Linear::var(&p.name).sub(&Linear::constant_only(1));
            type_constraints.push(Constraint(ge, Relation::Ge));
        }
    }

    // entry invariants for parameters whose type has an invariant
    let mut inv_constraints: Vec<Constraint> = Vec::new();
    for p in &func.params {
        if let Some(inv) = invariants.get(p.ty.name()) {
            for c in &inv.constraints {
                let inst = instantiate(c, &p.name);
                inv_constraints.extend(to_constraints(&inst, &pre_fn, &var_fn)?);
            }
        }
    }

    // Append type constraints and invariant constraints to every branch.
    for branch in &mut premise_dnf {
        branch.extend(type_constraints.iter().cloned());
        branch.extend(inv_constraints.iter().cloned());
    }

    // mutate state assignments -> equality premises defining post vars.
    // A top-level `if`/`else` in a function body splits into two branches
    // (guarded by the condition and its negation), each carrying its own
    // mutation constraints and set of assigned fields, so a conditional
    // mutation like `if flag == 1 { c.v += 1 } else { c.v = 0 }` produces two
    // separate verification branches instead of being silently ignored.
    let mut referenced: HashSet<(String, String)> = HashSet::new();
    for r in &func.requires {
        collect_fields(r, &mut referenced);
    }
    for e in &func.ensures {
        collect_fields(e, &mut referenced);
    }
    collect_body_referenced_fields(&func.body, &mut referenced);
    for p in &func.params {
        if let Some(fields) = invariant_fields.get(p.ty.name()) {
            for f in fields {
                referenced.insert((p.name.clone(), f.clone()));
            }
        }
    }

    let body_branches = process_body_stmts(
        &func.body,
        vec![BodyBranch {
            extra_premises: Vec::new(),
            mutations: Vec::new(),
            assigned: HashSet::new(),
        }],
        &pre_fn,
        &var_fn,
    )?;

    // Cartesian product: every premise branch paired with every body branch.
    // Each combined branch gets that body branch's extra (if-condition)
    // premises, its mutation constraints, and frame axioms computed from its
    // own assigned-field set (fields referenced but not assigned in *this*
    // branch keep their pre-state value).
    let mut combined_dnf: Vec<Vec<Constraint>> = Vec::new();
    for premise_branch in &premise_dnf {
        for body_branch in &body_branches {
            let mut branch = premise_branch.clone();
            branch.extend(body_branch.extra_premises.iter().cloned());
            branch.extend(body_branch.mutations.iter().cloned());
            for (base, field) in &referenced {
                if !body_branch
                    .assigned
                    .contains(&(base.clone(), field.clone()))
                {
                    let post = Linear::var(&post_field(base, field));
                    let pre = Linear::var(&pre_field(base, field));
                    branch.push(Constraint(post.sub(&pre), Relation::Eq));
                }
            }
            combined_dnf.push(branch);
        }
    }
    let premise_dnf = combined_dnf;

    // Build one VerificationProblem per DNF branch.
    let mut problems = Vec::new();
    for (idx, premises) in premise_dnf.iter().enumerate() {
        let premise_bounds = collect_var_bounds(premises);

        // conclusions: ensures + exit invariants
        let mut conclusions: Vec<Conclusion> = Vec::new();
        let mut next_or_group: usize = 0;
        for (i, e) in func.ensures.iter().enumerate() {
            let mut approximated = false;
            let dnf = to_constraints_bounded_dnf(
                e,
                &post_fn,
                &var_fn,
                &premise_bounds,
                &mut approximated,
            )?;
            let location = func.ensures_spans.get(i).copied().unwrap_or(func.span);
            if dnf.len() == 1 {
                // Single branch (conjunction) — one conclusion.
                for c in &dnf[0] {
                    conclusions.push(Conclusion {
                        description: format!("ensures: {}", pretty(e)),
                        constraint: c.clone(),
                        is_ensures: true,
                        is_approximation: approximated,
                        location,
                        or_group: None,
                    });
                }
            } else {
                // Disjunction: each branch is an alternative. At least one
                // branch must be entailed. We track them with a shared
                // `or_group` id so the verifier knows they form a disjunction.
                let group_id = next_or_group;
                next_or_group += 1;
                for (branch_idx, branch) in dnf.iter().enumerate() {
                    for c in branch {
                        conclusions.push(Conclusion {
                            description: format!("ensures: {} [branch {}]", pretty(e), branch_idx),
                            constraint: c.clone(),
                            is_ensures: true,
                            is_approximation: approximated,
                            location,
                            or_group: Some(group_id),
                        });
                    }
                }
            }
        }
        for p in &func.params {
            if let Some(inv) = invariants.get(p.ty.name()) {
                for (j, c) in inv.constraints.iter().enumerate() {
                    let inst = instantiate(c, &p.name);
                    let mut approximated = false;
                    let cs = to_constraints_bounded(
                        &inst,
                        &post_fn,
                        &var_fn,
                        &premise_bounds,
                        &mut approximated,
                    )?;
                    let location = inv.constraint_spans.get(j).copied().unwrap_or(inv.span);
                    for c2 in cs {
                        conclusions.push(Conclusion {
                            description: format!(
                                "invariant {} maintained: {}",
                                inv.name,
                                pretty(&inst)
                            ),
                            constraint: c2,
                            is_ensures: false,
                            is_approximation: approximated,
                            location,
                            or_group: None,
                        });
                    }
                }
            }
        }

        let func_name = if premise_dnf.len() == 1 {
            func.name.clone()
        } else {
            format!("{}[branch {}]", func.name, idx)
        };

        problems.push(VerificationProblem {
            func_name,
            func_span: func.span,
            premises: premises.clone(),
            conclusions,
        });
    }

    Ok(problems)
}
