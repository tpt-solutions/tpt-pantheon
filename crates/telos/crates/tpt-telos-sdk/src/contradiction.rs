//! Generic, `.telos`-source-independent contradiction checking over named
//! groups of linear-arithmetic constraints.
//!
//! Everything else in this crate orchestrates the full `.telos` pipeline
//! (parse -> transpile -> codegen -> attest). This module exposes just the
//! solver core (`tpt_telos_ir::Constraint` + `tpt_telos_verifier::unsat_checked`)
//! as a standalone building block: given named groups of constraints (e.g. one
//! group per alerting rule, or one group per input-shape assumption), find
//! which pairs can never simultaneously hold. A caller outside this repo that
//! wants this — but doesn't have `.telos` source to verify — can depend on
//! this crate, translate its own domain rules into [`Constraint`]s, and call
//! [`check_contradictions`] directly.

use tpt_telos_ir::Constraint;
use tpt_telos_verifier::unsat_checked;

/// One caller-named group of constraints to check for mutual consistency
/// against the other groups passed to [`check_contradictions`].
#[derive(Debug, Clone)]
pub struct NamedConstraints {
    pub label: String,
    pub constraints: Vec<Constraint>,
}

/// A pair of group labels whose combined constraints are jointly
/// unsatisfiable — the two groups can never both hold at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contradiction {
    pub a: String,
    pub b: String,
}

/// The result of [`check_contradictions`].
#[derive(Debug, Clone)]
pub struct ContradictionReport {
    /// Whether *every* group's constraints combined are jointly
    /// unsatisfiable. `None` when the solver could not decide (see
    /// [`tpt_telos_verifier::unsat_checked`]'s conservative overflow
    /// behavior) rather than a genuine yes/no answer.
    pub overall_unsat: Option<bool>,
    /// Every pair of groups whose combined constraints are jointly
    /// unsatisfiable.
    pub pairs: Vec<Contradiction>,
}

/// Find contradictions among named groups of linear-arithmetic constraints.
///
/// This only reasons over linear arithmetic (the same QF_LRA fragment the
/// core solver always has): a group with a nonlinear term (e.g. `area = h *
/// w`) must be pre-bounded into a linear over-approximation by the caller
/// before being passed in here.
///
/// # Examples
///
/// Two disjoint threshold rules on the same metric — mirroring an
/// alerting-rule DSL's `value <= 200` vs. `value >= 500` — are flagged as a
/// contradiction:
///
/// ```
/// use tpt_telos_ir::{Constraint, Linear, Relation};
/// use tpt_telos_sdk::{check_contradictions, NamedConstraints};
///
/// // value <= 200
/// let low = NamedConstraints {
///     label: "latency_ok".to_string(),
///     constraints: vec![Constraint(
///         Linear::var("value").sub(&Linear::constant_only(200)),
///         Relation::Le,
///     )],
/// };
/// // value >= 500
/// let high = NamedConstraints {
///     label: "latency_critical".to_string(),
///     constraints: vec![Constraint(
///         Linear::var("value").sub(&Linear::constant_only(500)),
///         Relation::Ge,
///     )],
/// };
///
/// let report = check_contradictions(&[low, high]);
/// // The two rules can never both hold (no value is <= 200 and >= 500).
/// assert_eq!(report.overall_unsat, Some(true));
/// assert_eq!(report.pairs.len(), 1);
/// assert_eq!(report.pairs[0].a, "latency_ok");
/// assert_eq!(report.pairs[0].b, "latency_critical");
/// ```
///
/// A threshold pair that *can* both hold — `value > 200` and `value >= 200`
/// (e.g. `value = 300` satisfies both) — is **not** a contradiction:
///
/// ```
/// use tpt_telos_ir::{Constraint, Linear, Relation};
/// use tpt_telos_sdk::{check_contradictions, NamedConstraints};
///
/// // value > 200  i.e.  value - 201 >= 0
/// let above = NamedConstraints {
///     label: "above".to_string(),
///     constraints: vec![Constraint(
///         Linear::var("value").sub(&Linear::constant_only(201)),
///         Relation::Ge,
///     )],
/// };
/// // value >= 200
/// let at_least = NamedConstraints {
///     label: "at_least".to_string(),
///     constraints: vec![Constraint(
///         Linear::var("value").sub(&Linear::constant_only(200)),
///         Relation::Ge,
///     )],
/// };
///
/// let report = check_contradictions(&[above, at_least]);
/// assert_eq!(report.overall_unsat, Some(false));
/// assert!(report.pairs.is_empty());
/// ```
pub fn check_contradictions(groups: &[NamedConstraints]) -> ContradictionReport {
    let all: Vec<Constraint> = groups
        .iter()
        .flat_map(|g| g.constraints.iter().cloned())
        .collect();
    let overall_unsat = unsat_checked(&all);

    let mut pairs = Vec::new();
    for i in 0..groups.len() {
        for j in (i + 1)..groups.len() {
            let mut combined = groups[i].constraints.clone();
            combined.extend(groups[j].constraints.iter().cloned());
            if unsat_checked(&combined) == Some(true) {
                pairs.push(Contradiction {
                    a: groups[i].label.clone(),
                    b: groups[j].label.clone(),
                });
            }
        }
    }

    ContradictionReport {
        overall_unsat,
        pairs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_telos_ir::{Linear, Relation};

    fn ge(var: &str, bound: i64) -> Constraint {
        Constraint(
            Linear::var(var).sub(&Linear::constant_only(bound)),
            Relation::Ge,
        )
    }

    fn le(var: &str, bound: i64) -> Constraint {
        Constraint(
            Linear::var(var).sub(&Linear::constant_only(bound)),
            Relation::Le,
        )
    }

    #[test]
    fn no_contradiction_among_consistent_groups() {
        let groups = vec![
            NamedConstraints {
                label: "a".to_string(),
                constraints: vec![ge("x", 0)],
            },
            NamedConstraints {
                label: "b".to_string(),
                constraints: vec![le("x", 100)],
            },
        ];
        let report = check_contradictions(&groups);
        assert_eq!(report.overall_unsat, Some(false));
        assert!(report.pairs.is_empty());
    }

    #[test]
    fn flags_pairwise_contradiction() {
        let groups = vec![
            NamedConstraints {
                label: "too_low".to_string(),
                constraints: vec![le("x", 0)],
            },
            NamedConstraints {
                label: "too_high".to_string(),
                constraints: vec![ge("x", 1)],
            },
        ];
        let report = check_contradictions(&groups);
        assert_eq!(report.overall_unsat, Some(true));
        assert_eq!(
            report.pairs,
            vec![Contradiction {
                a: "too_low".to_string(),
                b: "too_high".to_string(),
            }]
        );
    }

    #[test]
    fn only_conflicting_pair_is_reported_among_three() {
        // "low" (x <= 0) and "high" (x >= 1) conflict; "wide" (x >= -100) is
        // consistent with both individually.
        let groups = vec![
            NamedConstraints {
                label: "low".to_string(),
                constraints: vec![le("x", 0)],
            },
            NamedConstraints {
                label: "high".to_string(),
                constraints: vec![ge("x", 1)],
            },
            NamedConstraints {
                label: "wide".to_string(),
                constraints: vec![ge("x", -100)],
            },
        ];
        let report = check_contradictions(&groups);
        // The full set is unsatisfiable (low and high alone already conflict).
        assert_eq!(report.overall_unsat, Some(true));
        assert_eq!(
            report.pairs,
            vec![Contradiction {
                a: "low".to_string(),
                b: "high".to_string(),
            }]
        );
    }

    #[test]
    fn empty_groups_are_trivially_consistent() {
        let report = check_contradictions(&[]);
        assert_eq!(report.overall_unsat, Some(false));
        assert!(report.pairs.is_empty());
    }
}
