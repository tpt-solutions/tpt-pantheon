//! Integration test for `.telos` fixtures beyond `wallet`/`broken`.
//!
//! Exercises `nested.telos`, which contains a function with nested arithmetic
//! (constant multiplication, `old()` inside `ensures`) and another using `&&`
//! flattening in `requires`. Both are expected to verify.

use tpt_telos_ir::extract;
use tpt_telos_parser::parse;
use tpt_telos_verifier::verify;

#[test]
fn nested_example_passes() {
    let src = std::fs::read_to_string("../../examples/nested.telos").unwrap();
    let modules = parse(&src).unwrap();
    let problems = extract(&modules).unwrap();
    assert_eq!(problems.len(), 2);

    let mut by_name: std::collections::HashMap<_, _> = problems
        .into_iter()
        .map(|p| (p.func_name.clone(), p))
        .collect();

    for name in ["compound", "guarded"] {
        let p = by_name
            .remove(name)
            .unwrap_or_else(|| panic!("missing {name}"));
        let r = verify(&p);
        assert!(r.all_passed, "{name} should verify, got {:?}", r);
    }
}

#[test]
fn overflow_example_does_not_panic() {
    // examples/overflow.telos uses integer bounds at the i64 extremes. The
    // verifier must handle them with checked arithmetic (no panic, no spurious
    // contradiction) rather than overflowing fixed-width integers.
    let src = std::fs::read_to_string("../../examples/overflow.telos").unwrap();
    let modules = parse(&src).unwrap();
    let problems = extract(&modules).unwrap();
    assert_eq!(problems.len(), 1);
    let r = verify(&problems[0]);
    assert!(
        r.all_passed,
        "overflow.telos (a tautology) should verify, got {:?}",
        r
    );
}

#[test]
fn compound_has_expected_problem_shape() {
    let src = std::fs::read_to_string("../../examples/nested.telos").unwrap();
    let modules = parse(&src).unwrap();
    let problems = extract(&modules).unwrap();
    let compound = problems
        .iter()
        .find(|p| p.func_name == "compound")
        .expect("compound function present");
    // requires c.value >= 0, plus the PositiveInt constraints on `a`/`b`.
    assert!(compound.premises.len() >= 3);
    // one `ensures` clause (plus the maintained `Counter` invariant conclusion).
    assert_eq!(
        compound.conclusions.iter().filter(|c| c.is_ensures).count(),
        1
    );
    assert!(compound.conclusions.len() >= 2);
}

#[test]
fn disjunction_example_passes() {
    let src = std::fs::read_to_string("../../examples/disjunction.telos").unwrap();
    let modules = parse(&src).unwrap();
    let problems = extract(&modules).unwrap();
    // Disjunction in requires produces 2 premise branches -> 2 problems.
    assert_eq!(
        problems.len(),
        2,
        "expected 2 problems from requires disjunction"
    );

    for p in &problems {
        let r = verify(p);
        assert!(
            r.all_passed,
            "{} should verify (disjunction ensures), got {:?}",
            p.func_name, r
        );
    }
}

#[test]
fn cross_module_example_passes() {
    let src = std::fs::read_to_string("../../examples/cross_module.telos").unwrap();
    let modules = parse(&src).unwrap();
    let problems = extract(&modules).unwrap();
    assert_eq!(problems.len(), 2);

    for p in &problems {
        let r = verify(p);
        assert!(
            r.all_passed,
            "{} should verify (cross-module invariant), got {:?}",
            p.func_name, r
        );
    }
}

#[test]
fn real_time_example_passes() {
    // examples/real_time.telos: @boundary(real_time, zero_allocation) controller
    // (Rust backend) with a verified safe-range step.
    let src = std::fs::read_to_string("../../examples/real_time.telos").unwrap();
    let modules = parse(&src).unwrap();
    let problems = extract(&modules).unwrap();
    assert_eq!(problems.len(), 1);
    let r = verify(&problems[0]);
    assert!(r.all_passed, "real_time.telos should verify, got {:?}", r);
}

#[test]
fn crypto_example_passes() {
    // examples/crypto.telos: @boundary(crypto) secret store whose invariant is
    // preserved across consume/refresh.
    let src = std::fs::read_to_string("../../examples/crypto.telos").unwrap();
    let modules = parse(&src).unwrap();
    let problems = extract(&modules).unwrap();
    assert_eq!(problems.len(), 2);
    for p in &problems {
        let r = verify(p);
        assert!(
            r.all_passed,
            "{} should verify (crypto secret invariant), got {:?}",
            p.func_name, r
        );
    }
}

#[test]
fn cryptocurrency_example_passes() {
    // examples/cryptocurrency.telos: conservation invariant
    // (balance_a + balance_b == 1000000) preserved by a transfer.
    let src = std::fs::read_to_string("../../examples/cryptocurrency.telos").unwrap();
    let modules = parse(&src).unwrap();
    let problems = extract(&modules).unwrap();
    assert_eq!(problems.len(), 1);
    let r = verify(&problems[0]);
    assert!(
        r.all_passed,
        "cryptocurrency.telos should verify (coin conservation), got {:?}",
        r
    );
}

#[test]
fn distributed_example_passes() {
    // examples/distributed.telos: @boundary(distributed) coordinator (Go
    // backend) with a monotonically growing commit index.
    let src = std::fs::read_to_string("../../examples/distributed.telos").unwrap();
    let modules = parse(&src).unwrap();
    let problems = extract(&modules).unwrap();
    assert_eq!(problems.len(), 1);
    let r = verify(&problems[0]);
    assert!(r.all_passed, "distributed.telos should verify, got {:?}", r);
}
