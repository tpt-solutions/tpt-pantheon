//! Constraint-extraction (AST -> IR) unit tests for tpt-telos.
//!
//! Exercises the [`Linear`] arithmetic helpers and the [`extract`] function.
//! Covers `requires`/`ensures` lowering, `Positive*` parameter constraints,
//! invariant entry and maintenance, frame axioms, `old(...)` inside nested
//! arithmetic, and rejection of unsupported non-linear constructs.

use tpt_telos_ir::{extract, Constraint, Linear, Relation};
use tpt_telos_parser::parse;

// ---- helpers -------------------------------------------------------------

#[test]
fn linear_add_and_sub() {
    // (x + y) - (x + y) = 0  =>  empty terms
    let a = Linear::var("x").add(&Linear::var("y")); // x + y
    let b = Linear::var("x").add(&Linear::var("y")); // x + y
    let diff = a.sub(&b);
    assert!(diff.terms.is_empty());
    assert_eq!(diff.constant, 0);

    // x - (x + y) = -y
    let diff2 = Linear::var("x").sub(&a);
    assert_eq!(diff2.terms, vec![("y".to_string(), -1)]);
    assert_eq!(diff2.constant, 0);
}

#[test]
fn linear_scale_and_neg() {
    let s = Linear::var("x").scale(3);
    assert_eq!(s.terms, vec![("x".to_string(), 3)]);

    let n = Linear::var("x").neg();
    assert_eq!(n.terms, vec![("x".to_string(), -1)]);

    // scale distributes over the constant too
    let c = Linear::var("x").add(&Linear::constant_only(4)).scale(2);
    assert_eq!(c.terms, vec![("x".to_string(), 2)]);
    assert_eq!(c.constant, 8);

    // negating removes zero-coefficient terms
    let z = Linear::var("x").add(&Linear::var("x").neg());
    assert!(z.terms.is_empty());
}

#[test]
fn linear_constant_only() {
    let c = Linear::constant_only(7);
    assert!(c.terms.is_empty());
    assert_eq!(c.constant, 7);
}

// ---- extract --------------------------------------------------------------

fn first_problem(src: &str) -> tpt_telos_ir::VerificationProblem {
    let modules = parse(src).unwrap();
    let mut probs = extract(&modules).unwrap();
    assert_eq!(probs.len(), 1, "expected exactly one function problem");
    probs.remove(0)
}

// ---- source-span propagation (Phase 6, deliverable 1) --------------------

#[test]
fn conclusion_location_matches_source_ensures_span() {
    let src = "module M {\n\
               func f(x: i64)\n\
               requires x >= 0\n\
               ensures x >= 0\n\
               { }\n\
               }";
    let modules = parse(src).unwrap();
    let problems = extract(&modules).unwrap();
    assert_eq!(problems.len(), 1);
    let ensures_conclusion = problems[0]
        .conclusions
        .iter()
        .find(|c| c.is_ensures)
        .expect("expected an ensures conclusion");
    // The `ensures` clause is on line 4 of the fixture above.
    assert_eq!(ensures_conclusion.location.line, 4);
}

#[test]
fn conclusion_location_matches_invariant_constraint_span() {
    let src = "module M {\n\
               invariant Wallet {\n\
               balance >= 0\n\
               }\n\
               func f(w: Wallet)\n\
               { }\n\
               }";
    let modules = parse(src).unwrap();
    let problems = extract(&modules).unwrap();
    assert_eq!(problems.len(), 1);
    let invariant_conclusion = problems[0]
        .conclusions
        .iter()
        .find(|c| !c.is_ensures)
        .expect("expected a maintained-invariant conclusion");
    // The `balance >= 0` constraint is on line 3 of the fixture above --
    // distinct from the invariant's own `span` (line 2) and the function's
    // `func_span` (line 5), proving the location came from the invariant's
    // constraint, not a fallback.
    assert_eq!(invariant_conclusion.location.line, 3);
    assert_eq!(problems[0].func_span.line, 5);
}

#[test]
fn wallet_example_ensures_locations_match_fixture_lines() {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/wallet.telos"),
    )
    .expect("failed to read examples/wallet.telos");
    let modules = parse(&src).unwrap();
    let problems = extract(&modules).unwrap();
    assert_eq!(problems.len(), 1);
    let ensures_lines: Vec<usize> = problems[0]
        .conclusions
        .iter()
        .filter(|c| c.is_ensures)
        .map(|c| c.location.line)
        .collect();
    // The two `ensures` clauses sit on lines 19 and 20 of wallet.telos.
    assert_eq!(ensures_lines, vec![19, 20]);
}

#[test]
fn extract_requires_premise() {
    // requires x >= 0  =>  (x) - 0 >= 0
    let p = first_problem("module M { func f(x: i64) requires x >= 0 { } }");
    assert!(p.premises.iter().any(|Constraint(l, r)| {
        *r == Relation::Ge && l.terms == vec![("x".to_string(), 1)] && l.constant == 0
    }));
}

#[test]
fn extract_positive_param_constraint() {
    // A `Positive*` parameter adds the premise `x >= 1`.
    let p = first_problem("module M { func f(x: PositiveInt) { } }");
    assert!(p.premises.iter().any(|Constraint(l, r)| {
        *r == Relation::Ge && l.terms == vec![("x".to_string(), 1)] && l.constant == -1
    }));
}

#[test]
fn extract_invariant_premise_for_param() {
    // A parameter whose type has an invariant gets that invariant as a premise.
    let p = first_problem("module M { invariant Wallet { balance >= 0 } func f(w: Wallet) { } }");
    assert!(p.premises.iter().any(|Constraint(l, r)| {
        *r == Relation::Ge && l.terms == vec![("w.balance".to_string(), 1)] && l.constant == 0
    }));
}

#[test]
fn extract_maintained_invariant_conclusion() {
    // The invariant must be re-established in the post-state (is_ensures=false).
    let p = first_problem("module M { invariant Wallet { balance >= 0 } func f(w: Wallet) { } }");
    assert!(!p.conclusions.is_empty());
    assert!(p.conclusions.iter().any(|c| !c.is_ensures));
    assert!(p.conclusions.iter().any(|c| {
        if let Constraint(Linear { terms, .. }, Relation::Ge) = &c.constraint {
            terms == &[("w.balance'".to_string(), 1)]
        } else {
            false
        }
    }));
}

#[test]
fn extract_ensures_conclusion_on_field() {
    // A field in an `ensures` clause is lowered against the post-state name.
    let p = first_problem(
        "module M { invariant W { balance >= 0 } func f(w: W) ensures w.balance == 0 { } }",
    );
    assert!(p.conclusions.iter().any(|c| {
        c.is_ensures
            && if let Constraint(Linear { terms, .. }, Relation::Eq) = &c.constraint {
                terms.iter().any(|(n, _)| n == "w.balance'")
            } else {
                false
            }
    }));
}

#[test]
fn extract_ensures_conclusion_on_scalar() {
    // A scalar in an `ensures` clause is a plain variable (no post-state prime).
    let p = first_problem("module M { func f(x: i64) ensures x == 0 { } }");
    assert!(p.conclusions.iter().any(|c| {
        c.is_ensures
            && if let Constraint(Linear { terms, .. }, Relation::Eq) = &c.constraint {
                terms == &[("x".to_string(), 1)] && c.constraint.1 == Relation::Eq
            } else {
                false
            }
    }));
}

#[test]
fn extract_frame_axiom_for_unmutated_field() {
    // An unassigned struct field keeps its value across the call.
    let p = first_problem("module M { invariant W { balance >= 0 } func f(w: W) { } }");
    assert!(p.premises.iter().any(|Constraint(l, r)| {
        *r == Relation::Eq
            && l.terms.contains(&("w.balance'".to_string(), 1))
            && l.terms.contains(&("w.balance".to_string(), -1))
    }));
}

#[test]
fn extract_body_mutation_premise() {
    // `mutate state { w.balance -= amount }` sets w.balance' = w.balance - amount.
    let p = first_problem(
        "module M { invariant W { balance >= 0 } func f(w: W, amount: i64) { mutate state { w.balance -= amount } } }",
    );
    assert!(p.premises.iter().any(|Constraint(l, r)| {
        *r == Relation::Eq
            && l.terms.contains(&("w.balance'".to_string(), 1))
            && l.terms.contains(&("w.balance".to_string(), -1))
            && l.terms.contains(&("amount".to_string(), 1))
    }));
}

#[test]
fn extract_empty_contracts_ok() {
    // A function with no `requires`/`ensures` still lowers without error. An
    // invariant-bearing parameter contributes a maintained-invariant conclusion
    // but no `ensures` conclusion.
    let p = first_problem("module M { invariant W { balance >= 0 } func f(w: W) { } }");
    assert!(p.conclusions.iter().all(|c| !c.is_ensures));
}

#[test]
fn extract_old_in_arithmetic() {
    // ensures c.v == old(c.v) * 2  references the post-state field `c.v'`
    // and the pre-state field `c.v`.
    let p = first_problem(
        "module M { invariant C { v >= 0 } \
         func f(c: C) ensures c.v == old(c.v) * 2 { } }",
    );
    assert!(p.conclusions.iter().any(|c| {
        if let Constraint(Linear { terms, .. }, Relation::Eq) = &c.constraint {
            terms.iter().any(|(n, _)| n == "c.v'")
        } else {
            false
        }
    }));
    // No mutation, so a frame axiom keeps c.v' == c.v.
    assert!(p.premises.iter().any(|Constraint(l, r)| {
        *r == Relation::Eq && l.terms.contains(&("c.v'".to_string(), 1))
    }));
}

#[test]
fn extract_multiple_modules() {
    let src = "module A { func f(x: i64) requires x >= 0 { } }
                module B { func g(y: i64) ensures y == 0 { } }";
    let probs = extract(&parse(src).unwrap()).unwrap();
    assert_eq!(probs.len(), 2);
    assert_eq!(probs[0].func_name, "f");
    assert_eq!(probs[1].func_name, "g");
}

#[test]
fn extract_cross_module_invariant_reference() {
    // Module A defines the invariant; module B uses the type.
    let src = r#"
        module Types {
            invariant Wallet { balance >= 0 }
        }
        module Bank {
            func deposit(w: Wallet, amount: PositiveInt)
                requires amount > 0
                ensures w.balance == old(w.balance) + amount
            { mutate state { w.balance += amount } }
        }
    "#;
    let probs = extract(&parse(src).unwrap()).unwrap();
    // Should produce one problem for `deposit`.
    assert_eq!(probs.len(), 1);
    assert_eq!(probs[0].func_name, "deposit");
    // The invariant from module Types should be applied to the Wallet parameter.
    // This means the premises include `w.balance >= 0` (from the invariant).
    assert!(
        probs[0].premises.iter().any(|c| {
            c.0.terms.iter().any(|(v, _)| v == "w.balance") && c.1 == tpt_telos_ir::Relation::Ge
        }),
        "expected cross-module invariant premise w.balance >= 0: {:?}",
        probs[0].premises
    );
}

#[test]
fn extract_rejects_nonlinear_multiplication() {
    // `x * x` is non-linear and must be rejected during extraction.
    let src = "module M { func f(x: i64) ensures x * x == 0 { } }";
    assert!(extract(&parse(src).unwrap()).is_err());
}

#[test]
fn extract_rejects_division_by_variable() {
    // Division is only supported by a constant in constraints.
    let src = "module M { func f(x: i64, y: i64) ensures x / y == 0 { } }";
    assert!(extract(&parse(src).unwrap()).is_err());
}

#[test]
fn extract_rejects_division_by_zero() {
    let src = "module M { func f(x: i64) ensures x / 0 == 0 { } }";
    assert!(extract(&parse(src).unwrap()).is_err());
}

// ---- interval bounding (nonlinear products) ----

#[test]
fn extract_interval_bounding_replaces_product_with_constant() {
    // When both x and y have known bounds in requires, the nonlinear product
    // x * y in ensures is over-approximated via interval arithmetic.
    // x in [0,10], y in [0,5] => max(x*y) = 50.
    let src = r#"
        module M {
            func f(x: Int, y: Int)
                requires x >= 0
                requires x <= 10
                requires y >= 0
                requires y <= 5
                ensures x * y <= 50
            { }
        }
    "#;
    let probs = extract(&parse(src).unwrap()).unwrap();
    assert_eq!(probs.len(), 1);
    // The ensures conclusion should be marked as an approximation.
    let approx = probs[0]
        .conclusions
        .iter()
        .find(|c| c.is_ensures && c.is_approximation)
        .expect("expected an interval-bounded ensures conclusion");
    // The nonlinear product was replaced by its interval bound (50),
    // so the constraint is 50 - 50 <= 0, i.e. constant 0 with no variable terms.
    assert!(
        approx.constraint.0.terms.is_empty(),
        "interval-bounded constraint should be constant-only, got {:?}",
        approx.constraint.0
    );
    assert_eq!(
        approx.constraint.0.constant, 0,
        "interval-bounded constraint constant should be 0"
    );
}

#[test]
fn extract_interval_bounding_without_bounds_fails() {
    // Without bounds on both variables, nonlinear multiplication is rejected.
    let src = r#"
        module M {
            func f(x: Int, y: Int)
                ensures x * y <= 10
            { }
        }
    "#;
    assert!(extract(&parse(src).unwrap()).is_err());
}

#[test]
fn extract_interval_bounding_with_partial_bounds_fails() {
    // Only one variable bounded: nonlinear product still rejected.
    let src = r#"
        module M {
            func f(x: Int, y: Int)
                requires x >= 0
                requires x <= 10
                ensures x * y <= 50
            { }
        }
    "#;
    assert!(extract(&parse(src).unwrap()).is_err());
}

// ---- disjunction (||) in premises ----

#[test]
fn extract_disjunction_in_requires_splits_into_branches() {
    // `requires x > 0 || x < -10` produces two branches.
    let src = r#"
        module M {
            func f(x: Int)
                requires x > 0 || x < -10
                ensures x != 0
            { }
        }
    "#;
    let probs = extract(&parse(src).unwrap()).unwrap();
    // Two branches: one for x > 0, one for x < -10.
    assert_eq!(probs.len(), 2);
    assert!(probs[0].func_name.contains("branch 0"));
    assert!(probs[1].func_name.contains("branch 1"));
}

#[test]
fn extract_disjunction_with_conjunction_combines_correctly() {
    // `requires (x > 0 && y > 0) || (x < 0 && y < 0)` produces two branches,
    // each with two constraints from the conjunction.
    let src = r#"
        module M {
            func f(x: Int, y: Int)
                requires (x > 0 && y > 0) || (x < 0 && y < 0)
                ensures x != 0
            { }
        }
    "#;
    let probs = extract(&parse(src).unwrap()).unwrap();
    assert_eq!(probs.len(), 2);
    // Each branch has exactly 2 premise constraints (the conjunction members).
    assert_eq!(probs[0].premises.len(), 2);
    assert_eq!(probs[1].premises.len(), 2);
}

#[test]
fn extract_no_disjunction_produces_single_problem() {
    // Without ||, there's exactly one problem per function.
    let src = r#"
        module M {
            func f(x: Int)
                requires x >= 0
                ensures x >= 0
            { }
        }
    "#;
    let probs = extract(&parse(src).unwrap()).unwrap();
    assert_eq!(probs.len(), 1);
    // The func_name should not contain "branch".
    assert!(!probs[0].func_name.contains("branch"));
}
