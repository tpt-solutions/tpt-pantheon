//! Human/LLM-readable counterexample hints for verification failures.
//!
//! This is the "agent_hint" concept `tpt-nexus` flagged as missing from
//! tpt-telos: it turns a [`CheckResult`]'s clause kind, disjunction /
//! approximation caveats, and its counterexample [`Model`] into text an agent
//! (or a developer) can act on.

use tpt_telos_agent::FuncOutcome;
use tpt_telos_verifier::CheckResult;

/// Render one verification check as a hint line.
///
/// Failed checks include their counterexample, with post-state model keys
/// (e.g. `w.balance'`) rewritten to the readable `<base>.<field> (post-state)`
/// form. Disjunction-group membership and interval-bounded approximations are
/// called out explicitly so a consumer doesn't mistake a solver limitation for
/// a genuine spec violation.
///
/// # Examples
///
/// Basic rendering of a check:
///
/// ```
/// use tpt_telos_agent::{StaticAgent, transpile_module};
/// use tpt_telos_parser::parse;
/// use tpt_telos_sdk::format_hint;
///
/// let src = r#"
///     module M {
///         func f(c: Counter) ensures c.v == 0 ensures c.v == 1 ;
///     }
/// "#;
/// let modules = parse(src).unwrap();
/// let outcomes: Vec<_> = modules
///     .iter()
///     .flat_map(|m| transpile_module(m, &StaticAgent::new()).unwrap())
///     .collect();
/// let check = &outcomes[0].result.checks[0];
/// let hint = format_hint(check);
/// assert!(hint.contains("ensures"));
/// ```
///
/// A failing check surfaces its counterexample, with post-state keys rewritten
/// to `<base>.<field> (post-state)`:
///
/// ```
/// use std::collections::HashMap;
/// use tpt_telos_verifier::CheckResult;
/// use tpt_telos_sdk::format_hint;
///
/// let mut model = HashMap::new();
/// model.insert("c.v'".to_string(), 5);
/// let check = CheckResult {
///     description: "ensures c.v == 0".to_string(),
///     passed: false,
///     is_ensures: true,
///     is_approximation: false,
///     counterexample: Some(model),
///     or_group: None,
///     location: None,
/// };
/// let hint = format_hint(&check);
/// assert!(hint.contains("FAILED"));
/// assert!(hint.contains("c.v (post-state)=5"));
/// ```
pub fn format_hint(check: &CheckResult) -> String {
    let mut s = String::new();
    let kind = if check.is_ensures {
        "ensures"
    } else {
        "invariant"
    };
    s.push_str(&format!("[{}] {}", kind, check.description));
    if check.is_approximation {
        s.push_str(" (interval-bounded approximation)");
    }
    if let Some(group) = check.or_group {
        s.push_str(&format!(" (disjunction group {group})"));
    }
    if !check.passed {
        s.push_str(" — FAILED");
        if let Some(model) = &check.counterexample {
            s.push_str("\n    counterexample: ");
            let mut bindings: Vec<(&String, &i64)> = model.iter().collect();
            bindings.sort_by(|a, b| a.0.cmp(b.0));
            let parts: Vec<String> = bindings
                .iter()
                .map(|(k, v)| render_binding(k, **v))
                .collect();
            s.push_str(&parts.join(", "));
        }
    }
    s
}

/// Render a single counterexample binding, rewriting post-state keys
/// (`base.field'`) to `<base>.<field> (post-state)`.
fn render_binding(key: &str, value: i64) -> String {
    if let Some(stripped) = key.strip_suffix('\'') {
        format!("{} (post-state)={}", stripped, value)
    } else {
        format!("{}={}", key, value)
    }
}

/// Render every check of a transpilation outcome as hint lines, joined by
/// newlines. Returns an empty string only when the outcome has no checks; a
/// verified outcome still yields one line per (passing) check.
///
/// # Examples
///
/// ```
/// use tpt_telos_agent::{StaticAgent, transpile_module};
/// use tpt_telos_parser::parse;
/// use tpt_telos_sdk::format_outcome_hints;
///
/// let src = r#"
///     module Bank {
///         invariant Wallet { balance >= 0 }
///         func deposit(w: Wallet, amount: PositiveInt)
///             ensures w.balance == old(w.balance) + amount
///         ;
///     }
/// "#;
/// let modules = parse(src).unwrap();
/// let outcomes: Vec<_> = modules
///     .iter()
///     .flat_map(|m| transpile_module(m, &StaticAgent::new()).unwrap())
///     .collect();
/// let hints = format_outcome_hints(&outcomes[0]);
/// assert!(hints.contains("ensures"));
/// assert!(!hints.contains("FAILED"));
/// ```
pub fn format_outcome_hints(outcome: &FuncOutcome) -> String {
    outcome
        .result
        .checks
        .iter()
        .map(format_hint)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tpt_telos_verifier::CheckResult;

    fn failed_check() -> CheckResult {
        let mut model: HashMap<String, i64> = HashMap::new();
        model.insert("from.balance'".to_string(), 5);
        model.insert("from.balance".to_string(), 1);
        CheckResult {
            description: "ensures from.balance == old(from.balance) - amount".to_string(),
            passed: false,
            is_ensures: true,
            is_approximation: false,
            counterexample: Some(model),
            or_group: None,
            location: None,
        }
    }

    #[test]
    fn hint_reports_failure_and_post_state() {
        let hint = format_hint(&failed_check());
        assert!(hint.contains("FAILED"));
        assert!(hint.contains("counterexample"));
        // Post-state key is rendered readably.
        assert!(hint.contains("from.balance (post-state)=5"));
        // Pre-state key is rendered verbatim.
        assert!(hint.contains("from.balance=1"));
    }

    #[test]
    fn hint_notes_approximation_and_disjunction() {
        let mut c = failed_check();
        c.is_approximation = true;
        c.or_group = Some(2);
        let hint = format_hint(&c);
        assert!(hint.contains("interval-bounded approximation"));
        assert!(hint.contains("disjunction group 2"));
    }

    #[test]
    fn passing_check_has_no_counterexample_line() {
        let mut c = failed_check();
        c.passed = true;
        c.counterexample = None;
        let hint = format_hint(&c);
        assert!(!hint.contains("FAILED"));
        assert!(!hint.contains("counterexample"));
    }
}
