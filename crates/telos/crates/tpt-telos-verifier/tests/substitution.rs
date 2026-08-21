//! Tests for the equality-substitution preprocessing pass in the FM solver.
//!
//! These tests verify the canonical "small high-consequence numeric module"
//! patterns: ledger, rate-limiter, quota tracker, and state machine.

use tpt_telos_ir::{Constraint, Linear, Relation};
use tpt_telos_verifier::{entails, equality_substitute, unsat};

// ---------------------------------------------------------------------------
// Unit tests for equality_substitute itself
// ---------------------------------------------------------------------------

#[test]
fn substitute_single_equality() {
    // balance - old - deposit == 0, then balance >= 0
    // After substitution: old + deposit >= 0 (balance eliminated)
    let def = Constraint(
        Linear::var("balance")
            .sub(&Linear::var("old"))
            .sub(&Linear::var("deposit")),
        Relation::Eq,
    );
    let goal = Constraint(Linear::var("balance"), Relation::Ge);
    let mut cs = vec![def, goal];
    equality_substitute(&mut cs);

    assert_eq!(cs.len(), 1, "defining equality should be removed");
    let remaining = &cs[0];
    assert_eq!(remaining.1, Relation::Ge);
    assert!(
        remaining.0.terms.iter().any(|(v, _)| v == "old"),
        "old should appear in substituted constraint"
    );
    assert!(
        remaining.0.terms.iter().any(|(v, _)| v == "deposit"),
        "deposit should appear in substituted constraint"
    );
    assert!(
        remaining.0.terms.iter().all(|(v, _)| v != "balance"),
        "balance should have been eliminated"
    );
}

#[test]
fn substitute_chained_equalities() {
    // x == a + b  (x - a - b == 0)
    // y == x + c  (y - x - c == 0)
    // After substitution to fixpoint: y == a + b + c
    let def1 = Constraint(
        Linear::var("x").sub(&Linear::var("a")).sub(&Linear::var("b")),
        Relation::Eq,
    );
    let def2 = Constraint(
        Linear::var("y").sub(&Linear::var("x")).sub(&Linear::var("c")),
        Relation::Eq,
    );
    let goal = Constraint(Linear::var("y"), Relation::Ge); // y >= 0
    let mut cs = vec![def1, def2, goal];
    equality_substitute(&mut cs);

    // Only the goal should remain; both definitions are consumed.
    assert_eq!(cs.len(), 1, "both defining equalities should be removed");
    // y should be gone (substituted out)
    assert!(cs[0].0.terms.iter().all(|(v, _)| v != "y" && v != "x"));
}

#[test]
fn no_substitution_when_no_eq() {
    // No Eq constraint — nothing changes.
    let c1 = Constraint(Linear::var("x").sub(&Linear::constant_only(3)), Relation::Ge);
    let c2 = Constraint(Linear::var("x").sub(&Linear::constant_only(10)), Relation::Le);
    let mut cs = vec![c1.clone(), c2.clone()];
    equality_substitute(&mut cs);
    assert_eq!(cs.len(), 2);
}

#[test]
fn no_substitution_when_eq_has_no_unit_coefficient() {
    // 2*x == 4 (coefficient 2, not ±1) — cannot substitute cleanly.
    let def = Constraint(
        Linear {
            terms: vec![("x".to_string(), 2)],
            constant: -4,
        },
        Relation::Eq,
    );
    let goal = Constraint(Linear::var("x"), Relation::Ge);
    let mut cs = vec![def, goal];
    equality_substitute(&mut cs);
    // Should not have substituted — both constraints remain.
    assert_eq!(cs.len(), 2);
}

#[test]
fn substitute_negative_coefficient() {
    // -x + y == 0, i.e. x == y
    // goal: x >= 5  →  after substitution: y >= 5
    let def = Constraint(
        Linear {
            terms: vec![("x".to_string(), -1), ("y".to_string(), 1)],
            constant: 0,
        },
        Relation::Eq,
    );
    let goal = Constraint(
        Linear::var("x").sub(&Linear::constant_only(5)),
        Relation::Ge,
    );
    let mut cs = vec![def, goal];
    equality_substitute(&mut cs);
    assert_eq!(cs.len(), 1);
    assert!(cs[0].0.terms.iter().any(|(v, _)| v == "y"));
    assert!(cs[0].0.terms.iter().all(|(v, _)| v != "x"));
}

// ---------------------------------------------------------------------------
// Ledger pattern
// ---------------------------------------------------------------------------

#[test]
fn ledger_balance_conservation() {
    // balance = old_balance + deposit - withdrawal
    // requires: deposit >= 0, withdrawal >= 0, old_balance >= withdrawal
    // ensures:  balance >= 0
    //
    // Encoded as: prove (premises) → (balance >= 0)
    // where the "balance = ..." equality is a premise from the mutate state.

    // balance - old - deposit + withdrawal == 0
    let balance_def = Constraint(
        Linear::var("balance")
            .sub(&Linear::var("old"))
            .sub(&Linear::var("deposit"))
            .add(&Linear::var("withdrawal")),
        Relation::Eq,
    );
    // deposit >= 0
    let deposit_nonneg = Constraint(Linear::var("deposit"), Relation::Ge);
    // withdrawal >= 0
    let withdrawal_nonneg = Constraint(Linear::var("withdrawal"), Relation::Ge);
    // old >= withdrawal  (i.e. old - withdrawal >= 0)
    let old_ge_withdrawal = Constraint(
        Linear::var("old").sub(&Linear::var("withdrawal")),
        Relation::Ge,
    );

    let premises = vec![
        balance_def,
        deposit_nonneg,
        withdrawal_nonneg,
        old_ge_withdrawal,
    ];

    // conclusion: balance >= 0
    let conclusion = Constraint(Linear::var("balance"), Relation::Ge);
    assert!(
        entails(&premises, &conclusion),
        "ledger: balance >= 0 should follow from the premises"
    );
}

#[test]
fn ledger_balance_conservation_fails_without_old_ge_withdrawal() {
    // Without the requires old_balance >= withdrawal, we cannot prove balance >= 0.
    let balance_def = Constraint(
        Linear::var("balance")
            .sub(&Linear::var("old"))
            .sub(&Linear::var("deposit"))
            .add(&Linear::var("withdrawal")),
        Relation::Eq,
    );
    let deposit_nonneg = Constraint(Linear::var("deposit"), Relation::Ge);
    let withdrawal_nonneg = Constraint(Linear::var("withdrawal"), Relation::Ge);

    let premises = vec![balance_def, deposit_nonneg, withdrawal_nonneg];
    let conclusion = Constraint(Linear::var("balance"), Relation::Ge);
    assert!(
        !entails(&premises, &conclusion),
        "without old >= withdrawal, balance >= 0 should not be provable"
    );
}

// ---------------------------------------------------------------------------
// Rate-limiter pattern
// ---------------------------------------------------------------------------

#[test]
fn rate_limiter_tokens_nonneg() {
    // tokens = old_tokens - used
    // requires: used >= 0, used <= old_tokens
    // ensures:  tokens >= 0

    // tokens - old + used == 0
    let tokens_def = Constraint(
        Linear::var("tokens")
            .sub(&Linear::var("old"))
            .add(&Linear::var("used")),
        Relation::Eq,
    );
    // used >= 0
    let used_nonneg = Constraint(Linear::var("used"), Relation::Ge);
    // old - used >= 0  (used <= old_tokens)
    let used_le_old = Constraint(
        Linear::var("old").sub(&Linear::var("used")),
        Relation::Ge,
    );

    let premises = vec![tokens_def, used_nonneg, used_le_old];
    let conclusion = Constraint(Linear::var("tokens"), Relation::Ge);
    assert!(
        entails(&premises, &conclusion),
        "rate-limiter: tokens >= 0 should follow from the premises"
    );
}

// ---------------------------------------------------------------------------
// Quota tracker pattern
// ---------------------------------------------------------------------------

#[test]
fn quota_tracker_usage_within_limit() {
    // usage = old_usage + request
    // requires: request >= 0, old_usage + request <= quota
    // ensures:  usage <= quota

    // usage - old - request == 0
    let usage_def = Constraint(
        Linear::var("usage")
            .sub(&Linear::var("old"))
            .sub(&Linear::var("request")),
        Relation::Eq,
    );
    // request >= 0
    let request_nonneg = Constraint(Linear::var("request"), Relation::Ge);
    // quota - old - request >= 0  (old + request <= quota)
    let within_quota = Constraint(
        Linear::var("quota")
            .sub(&Linear::var("old"))
            .sub(&Linear::var("request")),
        Relation::Ge,
    );

    let premises = vec![usage_def, request_nonneg, within_quota];
    // ensures: quota - usage >= 0  (usage <= quota)
    let conclusion = Constraint(
        Linear::var("quota").sub(&Linear::var("usage")),
        Relation::Ge,
    );
    assert!(
        entails(&premises, &conclusion),
        "quota-tracker: usage <= quota should follow from the premises"
    );
}

// ---------------------------------------------------------------------------
// State machine: valid transition guard
// ---------------------------------------------------------------------------

#[test]
fn state_machine_valid_transition() {
    // next_state = current_state + delta
    // requires: delta == 1 (only +1 transitions allowed), current_state <= max_state - 1
    // ensures:  next_state <= max_state

    // next - current - delta == 0
    let next_def = Constraint(
        Linear::var("next")
            .sub(&Linear::var("current"))
            .sub(&Linear::var("delta")),
        Relation::Eq,
    );
    // delta - 1 == 0  (delta == 1)
    let delta_is_one = Constraint(
        Linear::var("delta").sub(&Linear::constant_only(1)),
        Relation::Eq,
    );
    // max - 1 - current >= 0  (current <= max - 1)
    let current_not_at_max = Constraint(
        Linear::var("max")
            .sub(&Linear::constant_only(1))
            .sub(&Linear::var("current")),
        Relation::Ge,
    );

    let premises = vec![next_def, delta_is_one, current_not_at_max];
    // ensures: max - next >= 0  (next <= max)
    let conclusion = Constraint(
        Linear::var("max").sub(&Linear::var("next")),
        Relation::Ge,
    );
    assert!(
        entails(&premises, &conclusion),
        "FSM: next_state <= max should follow from the transition guard"
    );
}

// ---------------------------------------------------------------------------
// Unsat sanity: substitution preserves contradiction detection
// ---------------------------------------------------------------------------

#[test]
fn substitution_preserves_contradiction() {
    // x == 5  (x - 5 == 0)
    // x <= 3  (x - 3 <= 0, i.e. x - 3 rel Le)
    // After substitution: 5 - 3 <= 0, i.e. 2 <= 0, which is False → UNSAT.
    let x_eq_5 = Constraint(
        Linear::var("x").sub(&Linear::constant_only(5)),
        Relation::Eq,
    );
    let x_le_3 = Constraint(
        Linear::var("x").sub(&Linear::constant_only(3)),
        Relation::Le,
    );
    assert!(
        unsat(&[x_eq_5, x_le_3]),
        "x == 5 and x <= 3 is a contradiction"
    );
}

#[test]
fn substitution_preserves_satisfiability() {
    // x == 2  and  x >= 0  is satisfiable (x = 2 is a witness).
    let x_eq_2 = Constraint(
        Linear::var("x").sub(&Linear::constant_only(2)),
        Relation::Eq,
    );
    let x_ge_0 = Constraint(Linear::var("x"), Relation::Ge);
    assert!(
        !unsat(&[x_eq_2, x_ge_0]),
        "x == 2 and x >= 0 should be satisfiable"
    );
}
