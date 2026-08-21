//! End-to-end integration tests for the `tpt-telos-sdk` pipeline.
//!
//! These exercise the headless `compile` / `compile_static` path only (no
//! `cargo`/`go` builds), mirroring how `tpt-nexus` drives the SDK.

use tpt_telos_ir::{Constraint, Linear, Relation};
use tpt_telos_sdk::{
    check_contradictions, compile, compile_static, format_outcome_hints, NamedConstraints,
    StaticAgent,
};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("../../examples/{name}"))
        .unwrap_or_else(|e| panic!("cannot read examples/{name}: {e}"))
}

#[test]
fn wallet_verifies_end_to_end() {
    let src = fixture("wallet.telos");
    let artifact = compile(&src, &StaticAgent::new()).unwrap();
    assert!(
        artifact.all_verified,
        "wallet.telos should be fully verified"
    );
}

#[test]
fn unsatisfiable_fails_with_contradiction_hint() {
    let src = fixture("unsatisfiable.telos");
    let artifact = compile(&src, &StaticAgent::new()).unwrap();
    assert!(
        !artifact.all_verified,
        "unsatisfiable.telos must not verify"
    );
    let hints = format_outcome_hints(&artifact.outcomes[0]);
    assert!(
        hints.contains("FAILED"),
        "hint should report the failure: {hints}"
    );
    assert!(
        hints.contains("counterexample"),
        "hint should surface the contradiction: {hints}"
    );
}

#[test]
fn broken_is_silently_repaired() {
    let src = fixture("broken.telos");
    let artifact = compile(&src, &StaticAgent::new()).unwrap();
    assert!(
        artifact.all_verified,
        "broken.telos should be silently repaired by the agentic loop"
    );
}

#[test]
fn compile_static_accepts_bytes() {
    let bytes = b"module Bank { func noop() ; }";
    let artifact = compile_static(bytes, &StaticAgent::new()).unwrap();
    assert!(artifact.all_verified);
}

/// `check_contradictions` is a `.telos`-source-independent entry point:
/// exercise it directly against a small alerting-rule-style set of named
/// constraint groups, the way a caller outside this repo (e.g. an alerting
/// engine with no `.telos` source) would use it.
#[test]
fn check_contradictions_flags_conflicting_rules_without_telos_source() {
    let cpu_critical = NamedConstraints {
        label: "cpu_critical".to_string(),
        constraints: vec![Constraint(
            Linear::var("cpu_pct").sub(&Linear::constant_only(90)),
            Relation::Gt,
        )],
    };
    let cpu_nominal = NamedConstraints {
        label: "cpu_nominal".to_string(),
        constraints: vec![Constraint(
            Linear::var("cpu_pct").sub(&Linear::constant_only(90)),
            Relation::Le,
        )],
    };
    let disk_full = NamedConstraints {
        label: "disk_full".to_string(),
        constraints: vec![Constraint(
            Linear::var("disk_pct").sub(&Linear::constant_only(95)),
            Relation::Ge,
        )],
    };

    let report = check_contradictions(&[cpu_critical, cpu_nominal, disk_full]);

    assert_eq!(
        report.overall_unsat,
        Some(true),
        "cpu_critical and cpu_nominal alone make the full set unsatisfiable"
    );
    assert_eq!(
        report.pairs.len(),
        1,
        "only the two cpu rules should conflict, not disk_full: {:?}",
        report.pairs
    );
    assert_eq!(report.pairs[0].a, "cpu_critical");
    assert_eq!(report.pairs[0].b, "cpu_nominal");
}
