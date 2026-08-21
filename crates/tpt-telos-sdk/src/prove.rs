//! `telos-prove`'s constraint-group schema, contradiction report, and output
//! formatters.
//!
//! Input is JSON describing one or more named constraint *groups* (e.g. one per
//! alerting rule, or one per input-shape assumption). Each group is a set of
//! linear-arithmetic [`Constraint`]s. We then:
//!
//! * detect any group that is **self-unsatisfiable** on its own
//!   (`unsat_checked` over just that group's constraints), and
//! * reuse [`check_contradictions`](crate::check_contradictions) to find
//!   pairwise conflicts *between* groups, plus an `overall_unsat` flag for the
//!   joint set (which catches jointly-unsat-but-no-pairwise-conflict inputs).
//!
//! The JSON schema (consumed by the `telos-prove` binary) is:
//!
//! ```text
//! {
//!   "groups": [
//!     {
//!       "label": "latency_ok",
//!       "constraints": [
//!         {
//!           "linear": { "terms": [["value", 1]], "constant": -200 },
//!           "relation": "<="
//!         }
//!       ]
//!     }
//!   ]
//! }
//! ```
//!
//! A bare top-level JSON *array* of groups is also accepted. `relation` is one
//! of `<=`, `>=`, `==`, `<`, `>`, `!=`. A `linear`'s `terms` are `[variable,
//! coefficient]` pairs; `constant` is added directly to the linear expression
//! (so `["value", 1]` with `constant: -200` and `<=` encodes `value <= 200`).
//!
//! This is pure I/O plumbing over the existing solver core — no new
//! `ConstraintSolver` trait, no new `Constraint` variants, no new `SatResult`
//! type (per the Phase 12 "stay self-contained" decision).

use tpt_telos_ir::{Constraint, Linear, Relation};

use crate::contradiction::{check_contradictions, NamedConstraints};
use crate::json::{parse_json, JsonError, JsonValue};

/// A neutral, machine-facing report of the contradiction check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProveReport {
    /// Whether the joint set of *all* constraints is unsatisfiable. `None` when
    /// the solver could not decide (conservative overflow behavior).
    pub overall_unsat: Option<bool>,
    /// Per-group results.
    pub groups: Vec<GroupResult>,
    /// Pairs of group labels whose combined constraints are jointly
    /// unsatisfiable (no pairwise conflict detected beyond the joint flag).
    pub contradictory_pairs: Vec<(String, String)>,
    /// Human-readable notes about schema/parse issues (non-fatal).
    pub warnings: Vec<String>,
}

/// Result for a single named group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupResult {
    pub label: String,
    /// True when the group's own constraints are self-contradictory (no joint
    /// reasoning with any other group needed).
    pub self_unsat: Option<bool>,
}

impl ProveReport {
    /// Render the report as human-readable, rustc-style text.
    pub fn format_human(&self) -> String {
        let mut out = String::new();
        for w in &self.warnings {
            out.push_str(&format!("warning: {w}\n"));
        }
        match self.overall_unsat {
            Some(true) => {
                out.push_str("overall: UNSATISFIABLE (the combined groups can never all hold)\n")
            }
            Some(false) => out.push_str("overall: satisfiable\n"),
            None => {
                out.push_str("overall: UNDECIDED (bounds too large for the FM solver to decide)\n")
            }
        }
        for g in &self.groups {
            let status = match g.self_unsat {
                Some(true) => "self-contradictory",
                Some(false) => "consistent (on its own)",
                None => "undecided (bounds too large)",
            };
            out.push_str(&format!("  group {}: {}\n", g.label, status));
        }
        if self.contradictory_pairs.is_empty() {
            out.push_str("  pairwise contradictions: none\n");
        } else {
            out.push_str("  pairwise contradictions:\n");
            for (a, b) in &self.contradictory_pairs {
                out.push_str(&format!("    {a} <-> {b}\n"));
            }
        }
        out
    }

    /// Render the report as JSON for CI/editors.
    pub fn format_json(&self) -> String {
        let mut s = String::new();
        s.push_str("{\n");
        s.push_str(&format!(
            "  \"overall_unsat\": {},\n",
            json_bool_opt(self.overall_unsat)
        ));
        s.push_str("  \"groups\": [\n");
        for (i, g) in self.groups.iter().enumerate() {
            s.push_str("    {\n");
            s.push_str(&format!("      \"label\": {},\n", json_str(&g.label)));
            s.push_str(&format!(
                "      \"self_unsat\": {}\n",
                json_bool_opt(g.self_unsat)
            ));
            s.push_str("    }");
            s.push_str(if i + 1 < self.groups.len() {
                ",\n"
            } else {
                "\n"
            });
        }
        s.push_str("  ],\n");
        s.push_str("  \"contradictory_pairs\": [\n");
        for (i, (a, b)) in self.contradictory_pairs.iter().enumerate() {
            s.push_str(&format!("    [{}, {}]", json_str(a), json_str(b)));
            s.push_str(if i + 1 < self.contradictory_pairs.len() {
                ",\n"
            } else {
                "\n"
            });
        }
        s.push_str("  ]\n");
        s.push_str("}\n");
        s
    }
}

fn json_bool_opt(b: Option<bool>) -> &'static str {
    match b {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Parse JSON text into named constraint groups.
///
/// Accepts either a top-level object with a `"groups"` array, or a bare
/// top-level array of groups.
pub fn parse_groups(src: &str) -> Result<Vec<NamedConstraints>, JsonError> {
    let v = parse_json(src)?;
    let arr = match &v {
        JsonValue::Array(a) => a,
        JsonValue::Object(_) => match v.get("groups") {
            Some(g) if g.is_array() => g.as_array(),
            Some(_) => {
                return Err(JsonError::new(
                    0,
                    "\"groups\" must be an array of group objects",
                ))
            }
            None => {
                return Err(JsonError::new(
                    0,
                    "top-level object must contain a \"groups\" array",
                ))
            }
        },
        _ => {
            return Err(JsonError::new(
                0,
                "top-level JSON must be an array of groups or an object with \"groups\"",
            ))
        }
    };
    let mut groups = Vec::new();
    for g in arr {
        if !g.is_object() {
            return Err(JsonError::new(0, "each group must be an object"));
        }
        let label = match g.get("label") {
            Some(JsonValue::Str(s)) => s.clone(),
            _ => return Err(JsonError::new(0, "group is missing a string \"label\"")),
        };
        let constraints = match g.get("constraints") {
            Some(c) if c.is_array() => c.as_array(),
            _ => {
                return Err(JsonError::new(
                    0,
                    format!("group {label:?} is missing a \"constraints\" array"),
                ))
            }
        };
        let mut cs = Vec::new();
        for c in constraints {
            cs.push(
                parse_constraint(c)
                    .map_err(|m| JsonError::new(0, format!("group {label:?}: {m}")))?,
            );
        }
        groups.push(NamedConstraints {
            label,
            constraints: cs,
        });
    }
    Ok(groups)
}

fn parse_constraint(v: &JsonValue) -> Result<Constraint, String> {
    if !v.is_object() {
        return Err("constraint must be an object".to_string());
    }
    let linear = v
        .get("linear")
        .ok_or_else(|| "constraint missing \"linear\"".to_string())?;
    if !linear.is_object() {
        return Err("\"linear\" must be an object".to_string());
    }
    let terms = linear
        .get("terms")
        .ok_or_else(|| "\"linear\" missing \"terms\"".to_string())?;
    if !terms.is_array() {
        return Err("\"terms\" must be an array".to_string());
    }
    let mut l = Linear::default();
    for t in terms.as_array() {
        if !t.is_array() || t.as_array().len() != 2 {
            return Err("each term must be a [variable, coefficient] pair".to_string());
        }
        let pair = t.as_array();
        let name = match &pair[0] {
            JsonValue::Str(s) => s.clone(),
            _ => return Err("term variable must be a string".to_string()),
        };
        let coeff = match &pair[1] {
            JsonValue::Number(n) => {
                if n.fract().abs() >= f64::EPSILON && *n != 0.0 {
                    return Err("term coefficient must be an integer".to_string());
                }
                *n as i64
            }
            _ => return Err("term coefficient must be a number".to_string()),
        };
        l.terms.push((name, coeff));
    }
    let constant = match linear.get("constant") {
        Some(JsonValue::Number(n)) => {
            if n.fract().abs() > f64::EPSILON {
                return Err("\"constant\" must be an integer".to_string());
            }
            *n as i64
        }
        Some(_) => return Err("\"constant\" must be a number".to_string()),
        None => 0,
    };
    l.constant = constant;

    let relation_str = match v.get("relation") {
        Some(JsonValue::Str(s)) => s.clone(),
        _ => return Err("constraint missing string \"relation\"".to_string()),
    };
    let relation = match relation_str.as_str() {
        "<=" => Relation::Le,
        ">=" => Relation::Ge,
        "==" => Relation::Eq,
        "<" => Relation::Lt,
        ">" => Relation::Gt,
        "!=" => Relation::Ne,
        other => return Err(format!("unknown relation {other:?}")),
    };
    Ok(Constraint(l, relation))
}

/// Build a [`ProveReport`] from parsed groups, running the self-unsat check per
/// group and the joint/pairwise check across groups.
pub fn build_report(groups: &[NamedConstraints]) -> ProveReport {
    let mut group_results = Vec::new();
    for g in groups {
        let self_unsat = tpt_telos_verifier::unsat_checked(&g.constraints);
        group_results.push(GroupResult {
            label: g.label.clone(),
            self_unsat,
        });
    }

    let contradiction = check_contradictions(groups);
    let contradictory_pairs = contradiction
        .pairs
        .iter()
        .map(|p| (p.a.clone(), p.b.clone()))
        .collect();

    ProveReport {
        overall_unsat: contradiction.overall_unsat,
        groups: group_results,
        contradictory_pairs,
        warnings: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_groups_from_top_level_array() {
        let src = r#"[
            {
                "label": "low",
                "constraints": [
                    { "linear": { "terms": [["x", 1]], "constant": 0 }, "relation": "<=" }
                ]
            },
            {
                "label": "high",
                "constraints": [
                    { "linear": { "terms": [["x", 1]], "constant": -1 }, "relation": ">=" }
                ]
            }
        ]"#;
        let groups = parse_groups(src).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].label, "low");
        assert_eq!(groups[0].constraints.len(), 1);
        assert_eq!(groups[0].constraints[0].1, Relation::Le);
    }

    #[test]
    fn parses_groups_from_object_with_groups_key() {
        let src = r#"{ "groups": [ { "label": "a", "constraints": [] } ] }"#;
        let groups = parse_groups(src).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].label, "a");
    }

    #[test]
    fn rejects_unknown_relation() {
        let src = r#"[ { "label": "a", "constraints": [ { "linear": { "terms": [] }, "relation": "~~" } ] } ]"#;
        let err = parse_groups(src).unwrap_err();
        assert!(err.message.contains("unknown relation"));
    }

    #[test]
    fn builds_report_flagging_pairwise_contradiction() {
        let groups = vec![
            NamedConstraints {
                label: "low".to_string(),
                constraints: vec![Constraint(
                    Linear::var("x").sub(&Linear::constant_only(0)),
                    Relation::Le,
                )],
            },
            NamedConstraints {
                label: "high".to_string(),
                constraints: vec![Constraint(
                    Linear::var("x").sub(&Linear::constant_only(1)),
                    Relation::Ge,
                )],
            },
        ];
        let report = build_report(&groups);
        assert_eq!(report.overall_unsat, Some(true));
        assert_eq!(
            report.contradictory_pairs,
            vec![("low".to_string(), "high".to_string())]
        );
        assert_eq!(report.groups[0].self_unsat, Some(false));
    }

    #[test]
    fn builds_report_flagging_self_unsat() {
        let groups = vec![NamedConstraints {
            label: "a".to_string(),
            constraints: vec![
                Constraint(
                    Linear::var("x").sub(&Linear::constant_only(1)),
                    Relation::Ge,
                ),
                Constraint(Linear::var("x"), Relation::Le),
            ],
        }];
        let report = build_report(&groups);
        assert_eq!(report.groups[0].self_unsat, Some(true));
    }

    #[test]
    fn human_and_json_format_are_stable() {
        let groups = vec![NamedConstraints {
            label: "low".to_string(),
            constraints: vec![Constraint(Linear::var("x"), Relation::Le)],
        }];
        let report = build_report(&groups);
        let human = report.format_human();
        assert!(human.contains("overall:"));
        assert!(human.contains("low"));
        let json = report.format_json();
        assert!(json.contains("\"overall_unsat\""));
        assert!(json.contains("\"low\""));
    }
}
