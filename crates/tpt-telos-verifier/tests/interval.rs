//! Integration test for nonlinear interval bounding.
//!
//! Exercises `interval.telos`, which contains a function with a nonlinear
//! product (`x * y`) in its `ensures` clause. Both operands have known bounds
//! from `requires`, so the verifier over-approximates via interval arithmetic
//! and marks the result `[interval-bounded]`.

use tpt_telos_ir::extract;
use tpt_telos_parser::parse;
use tpt_telos_verifier::verify;

#[test]
fn interval_bounding_verifies() {
    let src = std::fs::read_to_string("../../examples/interval.telos").unwrap();
    let modules = parse(&src).unwrap();
    let problems = extract(&modules).unwrap();
    assert_eq!(problems.len(), 1);

    let p = &problems[0];
    assert_eq!(p.func_name, "check");

    let result = verify(p);
    assert!(
        result.all_passed,
        "interval-bounded check should verify, got {:?}",
        result
    );
}

#[test]
fn interval_bounding_marks_approximation() {
    let src = std::fs::read_to_string("../../examples/interval.telos").unwrap();
    let modules = parse(&src).unwrap();
    let problems = extract(&modules).unwrap();
    let p = &problems[0];

    // The ensures conclusion involving x*y should be marked as an approximation.
    let ensures_checks: Vec<_> = p.conclusions.iter().filter(|c| c.is_ensures).collect();
    assert!(
        ensures_checks.iter().any(|c| c.is_approximation),
        "expected at least one interval-bounded ensures conclusion: {:?}",
        p.conclusions
    );
}

#[test]
fn interval_bounding_constant_replaces_product() {
    let src = std::fs::read_to_string("../../examples/interval.telos").unwrap();
    let modules = parse(&src).unwrap();
    let problems = extract(&modules).unwrap();
    let p = &problems[0];

    // The interval-bounded conclusion should have no variable terms
    // (the nonlinear product was replaced by a constant).
    let approx = p
        .conclusions
        .iter()
        .find(|c| c.is_ensures && c.is_approximation)
        .expect("expected an interval-bounded ensures conclusion");
    assert!(
        approx.constraint.0.terms.is_empty(),
        "interval-bounded constraint should be a constant, got {:?}",
        approx.constraint.0
    );
    // The constraint is 50 - 50 <= 0, i.e. constant 0.
    assert_eq!(approx.constraint.0.constant, 0);
}
