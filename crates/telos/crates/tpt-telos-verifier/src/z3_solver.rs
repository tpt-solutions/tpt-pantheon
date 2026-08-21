//! Z3-backed solver for tpt-telos constraints.
//!
//! This module is only available when the `z3` feature is enabled. It provides
//! an alternative solver backend that uses the Z3 SMT solver for exact
//! nonlinear arithmetic verification.
//!
//! Falls back to the built-in Fourier-Motzkin solver if Z3 is unavailable
//! at runtime.

use tpt_telos_ir::{Constraint, Relation};
use z3::{ast::Int, Config, Context, SatResult, Solver};

/// Check if Z3 is available at runtime by attempting to create a context.
pub fn is_z3_available() -> bool {
    Config::new().try_into().is_ok()
}

/// Convert a tpt-telos constraint into a Z3 integer AST.
fn constraint_to_z3<'a>(ctx: &'a Context, c: &Constraint) -> Int<'a> {
    let Constraint(lin, _rel) = c;
    let mut expr = Int::from_i64(ctx, lin.constant);
    for (var, coeff) in &lin.terms {
        let var_ast = Int::new_const(ctx, var.as_str());
        expr = &expr + &var_ast * coeff;
    }
    expr
}

/// Check if a set of constraints is unsatisfiable using Z3.
pub fn z3_unsat(cs: &[Constraint]) -> bool {
    let cfg = Config::new();
    let ctx = Context::new(&cfg);
    let solver = Solver::new(&ctx);

    for c in cs {
        let ast = constraint_to_z3(&ctx, c);
        let zero = Int::from_i64(&ctx, 0);
        match c.1 {
            Relation::Le => solver.assert(&ast <= zero),
            Relation::Lt => solver.assert(&ast < zero),
            Relation::Ge => solver.assert(&ast >= zero),
            Relation::Gt => solver.assert(&ast > zero),
            Relation::Eq => solver.assert(&ast._eq(&zero)),
            Relation::Ne => solver.assert(&ast._eq(&zero).not()),
        }
    }

    solver.check() == SatResult::Unsat
}

/// Find a model satisfying the constraints using Z3.
pub fn z3_model(cs: &[Constraint]) -> Option<std::collections::HashMap<String, i64>> {
    let cfg = Config::new();
    let ctx = Context::new(&cfg);
    let solver = Solver::new(&ctx);

    // Collect variable names.
    let mut var_names: Vec<String> = Vec::new();
    for c in cs {
        for (v, _) in &c.0.terms {
            if !var_names.contains(v) {
                var_names.push(v.clone());
            }
        }
    }

    for c in cs {
        let ast = constraint_to_z3(&ctx, c);
        let zero = Int::from_i64(&ctx, 0);
        match c.1 {
            Relation::Le => solver.assert(&ast <= zero),
            Relation::Lt => solver.assert(&ast < zero),
            Relation::Ge => solver.assert(&ast >= zero),
            Relation::Gt => solver.assert(&ast > zero),
            Relation::Eq => solver.assert(&ast._eq(&zero)),
            Relation::Ne => solver.assert(&ast._eq(&zero).not()),
        }
    }

    if solver.check() != SatResult::Sat {
        return None;
    }

    let model = solver.get_model()?;
    let mut result = std::collections::HashMap::new();
    for v in &var_names {
        let var_ast = Int::new_const(&ctx, v.as_str());
        if let Some(val) = model.eval(&var_ast, true) {
            if let Some(i) = val.as_i64() {
                result.insert(v.clone(), i);
            }
        }
    }
    Some(result)
}

/// Does `premises` entail `conclusion` using Z3?
pub fn z3_entails(premises: &[Constraint], concl: &Constraint) -> bool {
    use crate::solver::negate;
    for branch in negate(concl) {
        let mut combined: Vec<Constraint> = premises.to_vec();
        combined.extend(branch);
        if !z3_unsat(&combined) {
            return false;
        }
    }
    true
}
