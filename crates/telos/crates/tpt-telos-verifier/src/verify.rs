//! Top-level verification driver over a `VerificationProblem`.

#[cfg(feature = "z3")]
use crate::solver::negate;
use crate::solver::{counterexample, entails, unsat, Model};
use crate::SolverBackend;
use tpt_telos_ir::{Constraint, VerificationProblem};

/// The outcome of verifying one conclusion (an `ensures` clause or invariant).
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub description: String,
    pub passed: bool,
    pub is_ensures: bool,
    /// Mirrors [`tpt_telos_ir::Conclusion::is_approximation`]: true when the
    /// constraint was proved via interval-arithmetic bounding of a nonlinear
    /// product rather than exact linear arithmetic.
    pub is_approximation: bool,
    /// A concrete variable assignment that satisfies the premises but
    /// violates this conclusion. Only populated when `passed` is false, and
    /// only when the solver could construct one (always possible for the
    /// linear cases this verifier handles).
    pub counterexample: Option<Model>,
    /// Disjunction group: `Some(n)` when this check belongs to disjunction
    /// group `n` (at least one check in the group must pass). `None` for
    /// independent checks that must each pass individually.
    pub or_group: Option<usize>,
    /// Source location `(line, column)` of the conclusion (an `ensures` clause
    /// or invariant), used by the CLI to render rustc-style caret/underline
    /// diagnostics. `None` for synthetic checks not tied to a source span.
    pub location: Option<(usize, usize)>,
}

/// The aggregate verification result for a single function.
#[derive(Debug, Clone)]
pub struct VerificationResult {
    pub func_name: String,
    pub checks: Vec<CheckResult>,
    pub all_passed: bool,
}

/// Verify a single constraint against premises, dispatching to the active
/// solver backend (Z3 when the `z3` feature is enabled and the backend is set,
/// otherwise Fourier-Motzkin).
fn check_entails(premises: &[Constraint], concl: &Constraint) -> bool {
    #[cfg(feature = "z3")]
    {
        if crate::solver_backend() == crate::SolverBackend::Z3 {
            return crate::z3_solver::z3_entails(premises, concl);
        }
    }
    entails(premises, concl)
}

/// Extract a counterexample for a failing check, dispatching to the active
/// solver backend (Z3 when the `z3` feature is enabled and the backend is
/// set, otherwise Fourier-Motzkin).
fn check_counterexample(premises: &[Constraint], concl: &Constraint) -> Option<Model> {
    #[cfg(feature = "z3")]
    {
        if crate::solver_backend() == crate::SolverBackend::Z3 {
            for branch in negate(concl) {
                let mut cs = premises.to_vec();
                cs.extend(branch);
                if let Some(m) = crate::z3_solver::z3_model(&cs) {
                    return Some(m);
                }
            }
            return None;
        }
    }
    counterexample(premises, concl)
}

/// Verify every conclusion of a single function.
///
/// # Examples
///
/// ```
/// use tpt_telos_parser::parse;
/// use tpt_telos_ir::extract;
/// use tpt_telos_verifier::verify;
///
/// let src = r#"
///     module Bank {
///         invariant Wallet { balance >= 0 }
///         func deposit(w: Wallet, amount: PositiveInt)
///             requires amount > 0
///             ensures w.balance == old(w.balance) + amount
///         { mutate state { w.balance += amount } }
///     }
/// "#;
///
/// let modules = parse(src).unwrap();
/// let problems = extract(&modules).unwrap();
/// let result = verify(&problems[0]);
///
/// assert_eq!(result.func_name, "deposit");
/// assert!(result.all_passed);
/// assert!(result.checks.iter().all(|c| c.passed));
/// ```
pub fn verify(problem: &VerificationProblem) -> VerificationResult {
    use std::collections::HashMap;

    let mut checks = Vec::new();
    let mut all_passed = true;

    // First pass: check independent conclusions (or_group: None).
    for concl in &problem.conclusions {
        if concl.or_group.is_some() {
            continue; // handled in second pass
        }
        let passed = check_entails(&problem.premises, &concl.constraint);
        let ce = if passed {
            None
        } else {
            all_passed = false;
            check_counterexample(&problem.premises, &concl.constraint)
        };
        checks.push(CheckResult {
            description: concl.description.clone(),
            passed,
            is_ensures: concl.is_ensures,
            is_approximation: concl.is_approximation,
            counterexample: ce,
            or_group: concl.or_group,
            location: Some((concl.location.line, concl.location.col)),
        });
    }

    // Second pass: handle disjunction groups. For each group, at least one
    // conclusion must be entailed.
    let mut groups: HashMap<usize, Vec<&tpt_telos_ir::Conclusion>> = HashMap::new();
    for concl in &problem.conclusions {
        if let Some(g) = concl.or_group {
            groups.entry(g).or_default().push(concl);
        }
    }
    for group_conclusions in groups.values() {
        let mut any_passed = false;
        let mut group_results: Vec<CheckResult> = Vec::new();
        for concl in group_conclusions {
            let passed = check_entails(&problem.premises, &concl.constraint);
            if passed {
                any_passed = true;
            }
            let ce = if passed {
                None
            } else {
                check_counterexample(&problem.premises, &concl.constraint)
            };
            group_results.push(CheckResult {
                description: concl.description.clone(),
                passed,
                is_ensures: concl.is_ensures,
                is_approximation: concl.is_approximation,
                counterexample: ce,
                or_group: concl.or_group,
                location: Some((concl.location.line, concl.location.col)),
            });
        }
        if !any_passed {
            all_passed = false;
        }
        checks.extend(group_results);
    }

    VerificationResult {
        func_name: problem.func_name.clone(),
        checks,
        all_passed,
    }
}

/// Convenience: is this constraint set already contradictory?
///
/// # Examples
///
/// ```
/// use tpt_telos_ir::{Constraint, Linear, Relation};
/// use tpt_telos_verifier::is_unsat;
///
/// let ge1 = Constraint(Linear::var("x").sub(&Linear::constant_only(1)), Relation::Ge);
/// let le0 = Constraint(Linear::var("x"), Relation::Le);
/// assert!(is_unsat(&[ge1, le0]));
/// ```
pub fn is_unsat(cs: &[Constraint]) -> bool {
    match crate::solver_backend() {
        SolverBackend::FourierMotzkin => unsat(cs),
        #[cfg(feature = "z3")]
        SolverBackend::Z3 => crate::z3_solver::z3_unsat(cs),
    }
}
