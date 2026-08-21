//! A self-contained decision procedure for the quantifier-free linear
//! arithmetic fragment (QF_LRA) over the reals, which is a sound
//! over-approximation of integer arithmetic: if a set of constraints is
//! *unsatisfiable* over the reals, it is also unsatisfiable over the integers.
//!
//! This gives **sound** verification -- the solver will never report a
//! constraint as provable when it is not. (It may occasionally fail to prove a
//! true integer fact, i.e. it is incomplete, which is acceptable for Phase 1.)
//!
//! Elimination uses Fourier-Motzkin variable elimination.

use std::collections::HashMap;
use tpt_telos_ir::{Constraint, Relation};

#[derive(Clone, Debug)]
struct LinIneq {
    /// coefficients; the inequality is  sum(coeff * var) <= c
    coeffs: HashMap<String, i128>,
    c: i128,
}

fn neg_terms(terms: &[(String, i64)]) -> HashMap<String, i128> {
    terms
        .iter()
        .map(|(v, c)| (v.clone(), -(*c as i128)))
        .collect()
}

/// Convert a constraint into one or two `<= 0` style inequalities.
fn to_inequalities(cs: &[Constraint]) -> Vec<LinIneq> {
    let mut out = Vec::new();
    for Constraint(lin, rel) in cs {
        let const_i128 = lin.constant as i128;
        let base: HashMap<String, i128> = lin
            .terms
            .iter()
            .map(|(v, c)| (v.clone(), *c as i128))
            .collect();
        // All forms are normalised to `sum(coeff * var) <= c`.
        // Given `sum(t_i * var) + K  (rel)  0`:
        //   Le : sum t_i var <= -K
        //   Lt : sum t_i var <= -K - 1
        //   Ge : sum(-t_i) var <= K
        //   Gt : sum(-t_i) var <= K - 1
        //   Eq : (sum t_i var <= -K) and (sum(-t_i) var <= K)
        match rel {
            Relation::Le => out.push(LinIneq {
                coeffs: base,
                c: -const_i128,
            }),
            Relation::Lt => out.push(LinIneq {
                coeffs: base,
                c: -const_i128 - 1,
            }),
            Relation::Ge => out.push(LinIneq {
                coeffs: neg_terms(&lin.terms),
                c: const_i128,
            }),
            Relation::Gt => out.push(LinIneq {
                coeffs: neg_terms(&lin.terms),
                c: const_i128 - 1,
            }),
            Relation::Eq => {
                out.push(LinIneq {
                    coeffs: base.clone(),
                    c: -const_i128,
                });
                out.push(LinIneq {
                    coeffs: neg_terms(&lin.terms),
                    c: const_i128,
                });
            }
            // `!=` is only produced at the conclusion level; ignore it as a premise.
            Relation::Ne => {}
        }
    }
    out
}

fn remove_var(coeffs: &HashMap<String, i128>, v: &str) -> HashMap<String, i128> {
    coeffs
        .iter()
        .filter(|(k, _)| k.as_str() != v)
        .map(|(k, c)| (k.clone(), *c))
        .collect()
}

fn merge_keys(a: &HashMap<String, i128>, b: &HashMap<String, i128>) -> Vec<String> {
    let mut keys: Vec<String> = a.keys().cloned().collect();
    for k in b.keys() {
        if !keys.contains(k) {
            keys.push(k.clone());
        }
    }
    keys
}

/// Returns true iff the constraint set is unsatisfiable (over the reals).
///
/// This is **sound**: it will never report a set as unsatisfiable when it is
/// satisfiable. Coefficients are tracked in `i128`; if Fourier-Motzkin
/// elimination overflows `i128` on adversarial integer bounds, the elimination
/// is no longer trustworthy, so the result conservatively degrades to
/// *satisfiable* (i.e. "could not prove a contradiction") rather than risking a
/// spurious one. Callers that need to distinguish a genuine satisfiable result
/// from an overflow-aborted one should use [`unsat_checked`].
///
/// # Examples
///
/// ```
/// use tpt_telos_ir::{Constraint, Linear, Relation};
/// use tpt_telos_verifier::unsat;
///
/// // x >= 1  and  x <= 0  is a contradiction.
/// let ge1 = Constraint(Linear::var("x").sub(&Linear::constant_only(1)), Relation::Ge);
/// let le0 = Constraint(Linear::var("x"), Relation::Le);
/// assert!(unsat(&[ge1, le0]));
///
/// // x >= 0  alone is satisfiable.
/// let ge0 = Constraint(Linear::var("x"), Relation::Ge);
/// assert!(!unsat(&[ge0]));
/// ```
pub fn unsat(cs: &[Constraint]) -> bool {
    unsat_checked(cs).unwrap_or(false)
}

/// Like [`unsat`], but returns `None` when the `i128` coefficients overflow
/// during Fourier-Motzkin elimination and the result can no longer be trusted.
///
/// This is the explicit "bounds too large to decide" path: `Some(true)` means
/// the set is genuinely unsatisfiable, `Some(false)` means it is satisfiable,
/// and `None` means the constraints involve integer magnitudes beyond what the
/// exact linear solver can handle (use `--solver z3` for exact nonlinear
/// arithmetic, or scale the constants down). See [`unsat`] for the
/// conservative default used by the verification pipeline.
///
/// # Examples
///
/// ```
/// use tpt_telos_ir::{Constraint, Linear, Relation};
/// use tpt_telos_verifier::unsat_checked;
///
/// // x >= 1  and  x <= 0  is a contradiction.
/// let ge1 = Constraint(Linear::var("x").sub(&Linear::constant_only(1)), Relation::Ge);
/// let le0 = Constraint(Linear::var("x"), Relation::Le);
/// assert_eq!(unsat_checked(&[ge1, le0]), Some(true));
///
/// // x >= 0  alone is satisfiable.
/// let ge0 = Constraint(Linear::var("x"), Relation::Ge);
/// assert_eq!(unsat_checked(&[ge0]), Some(false));
/// ```
/// Preprocessing pass: substitute away equality-defined variables before FM elimination.
///
/// For each `Eq` constraint where some variable appears with coefficient ±1,
/// substitute that variable (isolated from the equality) into every other
/// constraint in the set, then remove the defining equality. Runs to fixpoint,
/// so chained equalities (`x == a + b`, `y == x + c`) are fully resolved.
///
/// This is sound and complete for QF_LRA equalities and makes proof chains
/// explicit, enabling the solver to verify patterns like:
/// ```text
/// balance = old(balance) + deposit - withdrawal
/// requires deposit >= 0 && old(balance) >= withdrawal
/// ensures balance >= 0   // proven: old(balance) + deposit - withdrawal >= 0
/// ```
///
/// # Examples
///
/// ```
/// use tpt_telos_ir::{Constraint, Linear, Relation};
/// use tpt_telos_verifier::equality_substitute;
///
/// // balance == old + deposit (encoded as balance - old - deposit == 0)
/// let def = Constraint(
///     Linear::var("balance")
///         .sub(&Linear::var("old"))
///         .sub(&Linear::var("deposit")),
///     Relation::Eq,
/// );
/// // ensures balance >= 0 (encoded as balance >= 0)
/// let goal = Constraint(Linear::var("balance"), Relation::Ge);
/// let mut cs = vec![def, goal];
/// equality_substitute(&mut cs);
/// // After substitution: [old + deposit >= 0]  (balance is eliminated)
/// assert_eq!(cs.len(), 1);
/// assert!(cs[0].0.terms.iter().any(|(v, _)| v == "old"));
/// ```
pub fn equality_substitute(cs: &mut Vec<Constraint>) {
    loop {
        // Find an Eq constraint that has at least one variable with coefficient ±1.
        let found = cs.iter().enumerate().find_map(|(i, c)| {
            if c.1 != Relation::Eq {
                return None;
            }
            c.0.terms
                .iter()
                .position(|(_, coeff)| *coeff == 1 || *coeff == -1)
                .map(|var_pos| (i, var_pos))
        });

        let Some((def_idx, var_pos)) = found else {
            break;
        };

        // Clone the defining equality and remove it from the set.
        let def = cs.remove(def_idx);
        let (var_name, coeff_x) = def.0.terms[var_pos].clone(); // coeff_x is ±1

        // Substitute `var_name → (-rest_terms - constant) / coeff_x` into every
        // remaining constraint that mentions `var_name`.
        //
        // For coeff_x = 1:  var = -sum(c_i * v_i) - K  → factor for each term: -c_i
        // For coeff_x = -1: var =  sum(c_i * v_i) + K  → factor for each term: +c_i
        // In both cases: added_coeff_for_v_i = d_x * (-c_i / coeff_x)
        // And: delta_constant = d_x * (-def.constant / coeff_x)
        for c in cs.iter_mut() {
            let Some(pos) = c.0.terms.iter().position(|(v, _)| *v == var_name) else {
                continue;
            };
            let d_x = c.0.terms[pos].1;
            c.0.terms.remove(pos);

            for (v, c_i) in &def.0.terms {
                if *v == var_name {
                    continue;
                }
                let added = d_x * (-c_i / coeff_x);
                if let Some(p) = c.0.terms.iter().position(|(u, _)| *u == *v) {
                    c.0.terms[p].1 += added;
                    if c.0.terms[p].1 == 0 {
                        c.0.terms.remove(p);
                    }
                } else {
                    c.0.terms.push((v.clone(), added));
                }
            }
            c.0.constant += d_x * (-def.0.constant / coeff_x);
        }
    }
}

pub fn unsat_checked(cs: &[Constraint]) -> Option<bool> {
    // Apply equality substitution first to reduce the constraint set before FM
    // elimination. This makes proof chains explicit and improves verification of
    // derived-value patterns (ledgers, rate-limiters, quota trackers).
    let mut cs_reduced = cs.to_vec();
    equality_substitute(&mut cs_reduced);
    let mut ineqs = to_inequalities(&cs_reduced);

    // collect variable names
    let mut vars: Vec<String> = Vec::new();
    for ineq in &ineqs {
        for k in ineq.coeffs.keys() {
            if !vars.contains(k) {
                vars.push(k.clone());
            }
        }
    }

    for v in &vars {
        let mut uppers: Vec<(i128, i128, HashMap<String, i128>)> = Vec::new();
        let mut lowers: Vec<(i128, i128, HashMap<String, i128>)> = Vec::new();
        let mut rest: Vec<LinIneq> = Vec::new();

        for ineq in &ineqs {
            match ineq.coeffs.get(v) {
                None => rest.push(ineq.clone()),
                Some(&tv) if tv > 0 => {
                    uppers.push((tv, ineq.c, remove_var(&ineq.coeffs, v)));
                }
                Some(&tv) if tv < 0 => {
                    lowers.push((-tv, -ineq.c, {
                        let mut m = remove_var(&ineq.coeffs, v);
                        for c in m.values_mut() {
                            *c = -*c;
                        }
                        m
                    }));
                }
                Some(_) => rest.push(ineq.clone()),
            }
        }

        let mut new_ineqs = rest;
        for (a, b, uc) in &uppers {
            for (e, d, lc) in &lowers {
                let keys = merge_keys(uc, lc);
                let mut coeffs = HashMap::new();
                for k in keys {
                    let bi = *uc.get(&k).unwrap_or(&0);
                    let ei = *lc.get(&k).unwrap_or(&0);
                    let coeff = e.checked_mul(bi)?.checked_sub(a.checked_mul(ei)?)?;
                    if coeff != 0 {
                        coeffs.insert(k, coeff);
                    }
                }
                let c = e.checked_mul(*b)?.checked_sub(a.checked_mul(*d)?)?;
                new_ineqs.push(LinIneq { coeffs, c });
            }
        }
        ineqs = new_ineqs;
    }

    for ineq in &ineqs {
        if ineq.coeffs.is_empty() && ineq.c < 0 {
            return Some(true);
        }
    }
    Some(false)
}

/// Negate a conclusion into one or more branches (each a conjunction of
/// constraints) that, conjoined with the premises, must each be unsatisfiable
/// for the conclusion to be entailed.
pub fn negate(concl: &Constraint) -> Vec<Vec<Constraint>> {
    let Constraint(lin, rel) = concl;
    let branch = |r: Relation| vec![Constraint(lin.clone(), r)];
    match rel {
        Relation::Eq => vec![branch(Relation::Lt), branch(Relation::Gt)],
        Relation::Ne => vec![branch(Relation::Eq)],
        Relation::Le => vec![branch(Relation::Gt)],
        Relation::Lt => vec![branch(Relation::Ge)],
        Relation::Ge => vec![branch(Relation::Lt)],
        Relation::Gt => vec![branch(Relation::Le)],
    }
}

/// Does `premises` entail `conclusion`?
///
/// # Examples
///
/// ```
/// use tpt_telos_ir::{Constraint, Linear, Relation};
/// use tpt_telos_verifier::entails;
///
/// // Given x >= 5, we can prove x >= 3.
/// let x_ge5 = Constraint(Linear::var("x").sub(&Linear::constant_only(5)), Relation::Ge);
/// let x_ge3 = Constraint(Linear::var("x").sub(&Linear::constant_only(3)), Relation::Ge);
/// assert!(entails(&[x_ge5], &x_ge3));
///
/// // But x >= 0 does not entail x >= 3.
/// let x_ge0 = Constraint(Linear::var("x"), Relation::Ge);
/// assert!(!entails(&[x_ge0], &x_ge3));
/// ```
pub fn entails(premises: &[Constraint], concl: &Constraint) -> bool {
    for branch in negate(concl) {
        let mut combined: Vec<Constraint> = premises.to_vec();
        combined.extend(branch);
        if !unsat(&combined) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Counter-example (model) extraction.
//
// When a conclusion is *not* entailed, we can construct a concrete witness: a
// variable assignment that satisfies the premises together with the negation of
// the conclusion. This witness is the "counter-example" fed back to the agentic
// code generator during the Verify -> Counter-example -> Rewrite loop.
// ---------------------------------------------------------------------------

/// A model maps variable names to integer values that satisfy a given
/// constraint set (when one exists).
///
/// # Examples
///
/// ```
/// use tpt_telos_ir::{Constraint, Linear, Relation};
/// use tpt_telos_verifier::model;
///
/// // x >= 3  and  x <= 5  →  model should assign x a value in [3, 5]
/// let ge3 = Constraint(Linear::var("x").sub(&Linear::constant_only(3)), Relation::Ge);
/// let le5 = Constraint(Linear::var("x").sub(&Linear::constant_only(5)), Relation::Le);
/// let m = model(&[ge3, le5]).expect("satisfiable");
/// let x = m["x"];
/// assert!(x >= 3 && x <= 5);
/// ```
pub type Model = std::collections::HashMap<String, i64>;

#[derive(Clone, Copy)]
struct Frac {
    num: i128,
    den: i128, // always > 0
}

impl Frac {
    fn new(num: i128, den: i128) -> Frac {
        debug_assert!(den != 0);
        let (num, den) = if den < 0 { (-num, -den) } else { (num, den) };
        let g = gcd(num.unsigned_abs(), den as u128);
        let g = g as i128;
        if g == 0 {
            Frac { num, den }
        } else {
            Frac {
                num: num / g,
                den: den / g,
            }
        }
    }

    /// Smallest integer >= self.
    fn ceil(self) -> i128 {
        let q = self.num.div_euclid(self.den);
        let r = self.num.rem_euclid(self.den);
        if r == 0 {
            q
        } else {
            q + 1
        }
    }

    /// Largest integer <= self.
    fn floor(self) -> i128 {
        self.num.div_euclid(self.den)
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

fn eval(coeffs: &HashMap<String, i128>, model: &Model) -> i128 {
    coeffs
        .iter()
        .map(|(v, c)| *c * model.get(v).copied().unwrap_or(0) as i128)
        .sum()
}

/// Find a concrete integer model satisfying `cs`, if one exists.
///
/// Uses Fourier-Motzkin variable elimination and reconstructs an integer
/// assignment from the derived bounds. Returns `None` if the system is
/// unsatisfiable (over the reals, hence over the integers).
///
/// # Examples
///
/// ```
/// use tpt_telos_ir::{Constraint, Linear, Relation};
/// use tpt_telos_verifier::model;
///
/// // x >= 2  and  x <= 4  →  some value in [2, 4]
/// let ge2 = Constraint(Linear::var("x").sub(&Linear::constant_only(2)), Relation::Ge);
/// let le4 = Constraint(Linear::var("x").sub(&Linear::constant_only(4)), Relation::Le);
/// let m = model(&[ge2, le4]).expect("satisfiable");
/// let x = m["x"];
/// assert!(x >= 2 && x <= 4);
///
/// // Contradictory constraints → no model.
/// let ge1 = Constraint(Linear::var("x").sub(&Linear::constant_only(1)), Relation::Ge);
/// let le0 = Constraint(Linear::var("x"), Relation::Le);
/// assert!(model(&[ge1, le0]).is_none());
/// ```
pub fn model(cs: &[Constraint]) -> Option<Model> {
    let ineqs = to_inequalities(cs);
    solve_model(ineqs)
}

fn solve_model(ineqs: Vec<LinIneq>) -> Option<Model> {
    // Collect variables.
    let mut vars: Vec<String> = Vec::new();
    for ineq in &ineqs {
        for k in ineq.coeffs.keys() {
            if !vars.contains(k) {
                vars.push(k.clone());
            }
        }
    }

    if vars.is_empty() {
        // Only constant inequalities remain; feasible iff every `c >= 0`.
        for ineq in &ineqs {
            if ineq.c < 0 {
                return None;
            }
        }
        return Some(Model::new());
    }

    let v = vars[0].clone();

    let mut uppers: Vec<(i128, i128, HashMap<String, i128>)> = Vec::new();
    let mut lowers: Vec<(i128, i128, HashMap<String, i128>)> = Vec::new();
    let mut rest: Vec<LinIneq> = Vec::new();

    for ineq in &ineqs {
        match ineq.coeffs.get(&v) {
            None => rest.push(ineq.clone()),
            Some(&tv) => {
                let others = remove_var(&ineq.coeffs, &v);
                if tv > 0 {
                    uppers.push((tv, ineq.c, others));
                } else if tv < 0 {
                    lowers.push((tv, ineq.c, others));
                } else {
                    rest.push(ineq.clone());
                }
            }
        }
    }

    // Combine every upper (a*v <= c - rest) with every lower (b*v <= c2 - rest2), b < 0.
    let mut new_ineqs = rest;
    for (a, cu, uc) in &uppers {
        for (b, c2, lc) in &lowers {
            let mut coeffs = HashMap::new();
            for k in merge_keys(uc, lc) {
                let bi = *uc.get(&k).unwrap_or(&0);
                let ei = *lc.get(&k).unwrap_or(&0);
                // derived earlier: a*rest2 - b*rest_u <= a*c2 - b*c
                let coeff = a * ei - b * bi;
                if coeff != 0 {
                    coeffs.insert(k, coeff);
                }
            }
            let c = a * c2 - b * cu;
            new_ineqs.push(LinIneq { coeffs, c });
        }
    }

    let mut model = solve_model(new_ineqs)?;

    // Recover a value for `v` from its (real) bounds.
    let mut low: Option<Frac> = None;
    for (b, c2, lc) in &lowers {
        // b*v <= c2 - lc  with b < 0  =>  v >= (lc_val - c2)/(-b)
        let lc_val = eval(lc, &model);
        let num = lc_val - c2;
        let den = -b; // > 0
        let f = Frac::new(num, den);
        low = Some(match low {
            None => f,
            Some(x) => {
                if f.num * x.den >= x.num * f.den {
                    f
                } else {
                    x
                }
            }
        });
    }

    let mut high: Option<Frac> = None;
    for (a, cu, uc) in &uppers {
        // a*v <= cu - uc  with a > 0  =>  v <= (cu - uc_val)/a
        let uc_val = eval(uc, &model);
        let num = cu - uc_val;
        let den = *a; // > 0
        let f = Frac::new(num, den);
        high = Some(match high {
            None => f,
            Some(x) => {
                if f.num * x.den <= x.num * f.den {
                    f
                } else {
                    x
                }
            }
        });
    }

    let vval: i128 = match (low, high) {
        (None, None) => 0,
        (Some(l), None) => l.ceil(),
        (None, Some(h)) => h.floor(),
        (Some(l), Some(h)) => {
            let lo = l.ceil();
            let hi = h.floor();
            if lo > hi {
                return None;
            }
            lo
        }
    };

    model.insert(v, vval as i64);
    Some(model)
}

/// Produce a concrete counter-example (a witness model) showing that
/// `conclusion` does *not* follow from `premises`. Returns `None` when the
/// conclusion is actually entailed (no counter-example exists).
///
/// # Examples
///
/// ```
/// use tpt_telos_ir::{Constraint, Linear, Relation};
/// use tpt_telos_verifier::counterexample;
///
/// // x >= 0 does not entail x >= 5; a counter-example exists.
/// let x_ge0 = Constraint(Linear::var("x"), Relation::Ge);
/// let x_ge5 = Constraint(Linear::var("x").sub(&Linear::constant_only(5)), Relation::Ge);
/// let ce = counterexample(&[x_ge0], &x_ge5)
///     .expect("counter-example exists");
/// assert!(ce["x"] < 5);
///
/// // When the conclusion is entailed, no counter-example exists.
/// let x_ge10 = Constraint(Linear::var("x").sub(&Linear::constant_only(10)), Relation::Ge);
/// assert!(counterexample(&[x_ge10], &x_ge5).is_none());
/// ```
pub fn counterexample(premises: &[Constraint], concl: &Constraint) -> Option<Model> {
    for branch in negate(concl) {
        let mut cs = premises.to_vec();
        cs.extend(branch);
        if let Some(m) = model(&cs) {
            return Some(m);
        }
    }
    None
}

/// Check whether an integer model satisfies every constraint in `cs`.
/// Used to validate a generated counter-example before handing it to the agent.
///
/// # Examples
///
/// ```
/// use tpt_telos_ir::{Constraint, Linear, Relation};
/// use tpt_telos_verifier::satisfies_model;
/// use std::collections::HashMap;
///
/// // x >= 3 satisfied by x = 5
/// let ge3 = Constraint(Linear::var("x").sub(&Linear::constant_only(3)), Relation::Ge);
/// let mut m = HashMap::new();
/// m.insert("x".to_string(), 5_i64);
/// assert!(satisfies_model(&[ge3.clone()], &m));
///
/// // x >= 3 not satisfied by x = 1
/// m.insert("x".to_string(), 1_i64);
/// assert!(!satisfies_model(&[ge3], &m));
/// ```
pub fn satisfies_model(cs: &[Constraint], model: &Model) -> bool {
    let ineqs = to_inequalities(cs);
    for ineq in &ineqs {
        let lhs: i128 = ineq
            .coeffs
            .iter()
            .map(|(v, c)| *c * model.get(v).copied().unwrap_or(0) as i128)
            .sum();
        if lhs > ineq.c {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use tpt_telos_ir::Linear;

    fn c(terms: &[(&str, i64)], k: i64, rel: Relation) -> Constraint {
        Constraint(
            Linear {
                terms: terms.iter().map(|(v, c)| (v.to_string(), *c)).collect(),
                constant: k,
            },
            rel,
        )
    }

    #[test]
    fn model_finds_witness() {
        // 1 <= x <= 3, y == x + 1
        let cs = vec![
            c(&[("x", 1)], -1, Relation::Ge),
            c(&[("x", 1)], -3, Relation::Le),
            c(&[("y", 1), ("x", -1)], -1, Relation::Eq),
        ];
        let m = model(&cs).expect("should be satisfiable");
        assert!(satisfies_model(&cs, &m), "model {m:?} invalid");
    }

    #[test]
    fn model_unsat_none() {
        // x >= 1 && x <= 0
        let cs = vec![
            c(&[("x", 1)], -1, Relation::Ge),
            c(&[("x", 1)], 0, Relation::Le),
        ];
        assert!(model(&cs).is_none());
    }

    #[test]
    fn model_counterexample_for_postcondition() {
        // premises: y' == y - 1 ; y >= 0
        // conclusion (false): y' >= y
        let pre = vec![
            c(&[("y'", 1), ("y", -1)], 1, Relation::Eq),
            c(&[("y", 1)], 0, Relation::Ge),
        ];
        let concl = c(&[("y'", 1), ("y", -1)], 0, Relation::Ge);
        for branch in negate(&concl) {
            let mut combined = pre.clone();
            combined.extend(branch);
            let m = model(&combined).expect("counterexample should exist");
            assert!(satisfies_model(&combined, &m));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_telos_ir::Linear;

    fn c(terms: &[(&str, i64)], k: i64, rel: Relation) -> Constraint {
        Constraint(
            Linear {
                terms: terms.iter().map(|(v, c)| (v.to_string(), *c)).collect(),
                constant: k,
            },
            rel,
        )
    }

    #[test]
    fn unsat_obvious() {
        // x <= 0 && x >= 1
        let cs = vec![
            c(&[("x", 1)], 0, Relation::Le),
            c(&[("x", 1)], -1, Relation::Ge),
        ];
        assert!(unsat(&cs));
    }

    #[test]
    fn sat_consistent_bounds() {
        // 0 <= x <= 5
        let cs = vec![
            c(&[("x", 1)], 0, Relation::Ge),
            c(&[("x", 1)], -5, Relation::Le),
        ];
        assert!(!unsat(&cs));
    }

    #[test]
    fn entails_simple() {
        // premises: x >= 1  =>  x >= 0
        let pre = vec![c(&[("x", 1)], -1, Relation::Ge)];
        let concl = c(&[("x", 1)], 0, Relation::Ge);
        assert!(entails(&pre, &concl));
    }

    #[test]
    fn entails_negative() {
        // premises: x >= 1  does NOT entail x >= 2
        let pre = vec![c(&[("x", 1)], -1, Relation::Ge)];
        let concl = c(&[("x", 1)], -2, Relation::Ge);
        assert!(!entails(&pre, &concl));
    }

    #[test]
    fn entails_affine_postcondition() {
        // y == x + 1, x >= 0  =>  y >= 1
        let pre = vec![
            c(&[("y", 1), ("x", -1)], -1, Relation::Eq),
            c(&[("x", 1)], 0, Relation::Ge),
        ];
        let concl = c(&[("y", 1)], -1, Relation::Ge);
        assert!(entails(&pre, &concl));
    }
}

#[cfg(test)]
mod extended_tests {
    use super::*;
    use tpt_telos_ir::Linear;

    fn c(terms: &[(&str, i64)], k: i64, rel: Relation) -> Constraint {
        Constraint(
            Linear {
                terms: terms.iter().map(|(v, c)| (v.to_string(), *c)).collect(),
                constant: k,
            },
            rel,
        )
    }

    #[test]
    fn unsat_three_variables() {
        // x >= 1 && x <= 0  (impossible regardless of y, z)
        let cs = vec![
            c(&[("x", 1)], -1, Relation::Ge),
            c(&[("x", 1)], 0, Relation::Le),
        ];
        assert!(unsat(&cs));
    }

    #[test]
    fn unsat_with_mixed_relations() {
        // x <= 0 && x >= 2 && y == x + 1
        let cs = vec![
            c(&[("x", 1)], 0, Relation::Le),
            c(&[("x", 1)], -2, Relation::Ge),
            c(&[("y", 1), ("x", -1)], -1, Relation::Eq),
        ];
        assert!(unsat(&cs));
    }

    #[test]
    fn sat_three_variable_bounds() {
        // 0 <= x <= 5, 0 <= y <= 3, z == x + y  (feasible)
        let cs = vec![
            c(&[("x", 1)], 0, Relation::Ge),
            c(&[("x", 1)], -5, Relation::Le),
            c(&[("y", 1)], 0, Relation::Ge),
            c(&[("y", 1)], -3, Relation::Le),
            c(&[("z", 1), ("x", -1), ("y", -1)], 0, Relation::Eq),
        ];
        assert!(!unsat(&cs));
    }

    #[test]
    fn entails_with_neq_conclusion() {
        // premises: x >= 1  does NOT entail x != 0  (x == 1 satisfies x>=1 and x!=0;
        // but x could also be 1, which is != 0, so conclusion holds? No: we must
        // prove it for ALL x>=1. x>=1 implies x!=0, so it IS entailed.)
        let pre = vec![c(&[("x", 1)], -1, Relation::Ge)];
        let concl = c(&[("x", 1)], 0, Relation::Ne);
        assert!(entails(&pre, &concl));
    }

    #[test]
    fn entails_neq_negative() {
        // premises: x >= 0  does NOT entail x != 1  (x == 1 breaks it)
        let pre = vec![c(&[("x", 1)], 0, Relation::Ge)];
        let concl = c(&[("x", 1)], -1, Relation::Ne);
        assert!(!entails(&pre, &concl));
    }

    #[test]
    fn counterexample_finds_witness_for_failed_postcondition() {
        // premises: y' == y - 1 ; y >= 0
        // conclusion (false): y' >= y
        let pre = vec![
            c(&[("y'", 1), ("y", -1)], 1, Relation::Eq),
            c(&[("y", 1)], 0, Relation::Ge),
        ];
        let concl = c(&[("y'", 1), ("y", -1)], 0, Relation::Ge);
        let ce = counterexample(&pre, &concl).expect("a counter-example must exist");
        // The witness must satisfy the premises together with the negated
        // conclusion (y' < y, i.e. y' - y + 1 <= 0).
        let negated = vec![c(&[("y'", 1), ("y", -1)], 1, Relation::Le)];
        let mut combined = pre.clone();
        combined.extend(negated);
        assert!(
            satisfies_model(&combined, &ce),
            "counter-example {ce:?} invalid"
        );
    }

    #[test]
    fn counterexample_none_when_entailed() {
        // premises: y' == y - 1 ; y >= 0  =>  y' <= y  (entailed, no CE)
        let pre = vec![
            c(&[("y'", 1), ("y", -1)], 1, Relation::Eq),
            c(&[("y", 1)], 0, Relation::Ge),
        ];
        let concl = c(&[("y'", 1), ("y", -1)], 0, Relation::Le);
        assert!(counterexample(&pre, &concl).is_none());
    }

    #[test]
    fn model_solves_linear_division() {
        // 2 * x == 4  =>  x == 2
        let cs = vec![c(&[("x", 2)], -4, Relation::Eq)];
        let m = model(&cs).expect("should be satisfiable");
        assert_eq!(m.get("x").copied().unwrap_or(0), 2);
        assert!(satisfies_model(&cs, &m));
    }

    #[test]
    fn integer_overflow_edge_bounds() {
        // Bounds at the i64 extremes are handled (the solver works in i128).
        let max = i64::MAX;
        // x >= i64::MAX && x <= i64::MAX - 1  =>  unsat
        let cs = vec![
            c(&[("x", 1)], -max, Relation::Ge),
            c(&[("x", 1)], -(max - 1), Relation::Le),
        ];
        assert!(unsat(&cs));
        // x <= i64::MAX  =>  sat  (the lower extreme is trivially true for i64)
        let sat = vec![c(&[("x", 1)], -max, Relation::Le)];
        assert!(!unsat(&sat));
    }

    #[test]
    fn entails_across_i64_extremes() {
        // x == i64::MAX  =>  x >= i64::MAX
        let max = i64::MAX;
        let pre = vec![c(&[("x", 1)], -max, Relation::Eq)];
        let concl = c(&[("x", 1)], -max, Relation::Ge);
        assert!(entails(&pre, &concl));
        // but x == i64::MAX  does NOT entail  x >= 0  is... actually it does
        // (MAX >= 0). Instead show it does NOT entail x <= 0.
        let concl2 = c(&[("x", 1)], 0, Relation::Le);
        assert!(!entails(&pre, &concl2));
    }

    #[test]
    fn model_respects_all_relations() {
        // Independent integer bounds on two variables; any corner satisfies all.
        let cs = vec![
            c(&[("x", 1)], -1, Relation::Ge),
            c(&[("x", 1)], -3, Relation::Le),
            c(&[("y", 1)], -2, Relation::Ge),
            c(&[("y", 1)], -4, Relation::Le),
            c(&[("x", 1), ("y", 1)], -100, Relation::Le),
        ];
        let m = model(&cs).expect("should be satisfiable");
        assert!(satisfies_model(&cs, &m), "model {m:?} invalid");
    }

    #[test]
    fn unsat_checked_overflow_is_conservative() {
        // Chain of variables each bounded to zero by `>= 0 && <= 0`, linked by
        // `-m*v_i - m*v_{i+1} <= 0` constraints. Eliminating the linked variable
        // multiplies an `i128` coefficient near `i64::MAX^2` by another
        // `i64::MAX`, which overflows `i128`. The contradiction (last var >= 1)
        // is genuine, but fixed-width Fourier-Motzkin can no longer be trusted,
        // so `unsat_checked` must report `None` (undecided) and `unsat` must not
        // claim a (possibly spurious) unsatisfiability.
        let m = i64::MAX;
        let n = 5; // variables v0..v4
        let mut cs = Vec::new();
        for i in 0..n {
            let vi = format!("v{i}");
            // v_i >= 0  (lower for v_i)
            cs.push(c(&[(vi.as_str(), -m)], 0, Relation::Le));
            // v_i <= 0  (upper for v_i) — together force v_i == 0
            cs.push(c(&[(vi.as_str(), m)], 0, Relation::Le));
            if i + 1 < n {
                let vj = format!("v{}", i + 1);
                // m*v_i + m*v_{i+1} <= 0  — an UPPER for v_i that chains a huge
                // coefficient forward so the next elimination multiplies an
                // `i128` near `i64::MAX^2` by another `i64::MAX` (overflows).
                cs.push(c(&[(vi.as_str(), m), (vj.as_str(), m)], 0, Relation::Le));
            }
        }
        // contradiction: v_{n-1} >= 1  (but all v_i are forced to 0)
        let last = format!("v{}", n - 1);
        cs.push(c(&[(last.as_str(), -m)], m, Relation::Le));

        assert_eq!(
            unsat_checked(&cs),
            None,
            "overflow must degrade to undecided"
        );
        assert!(
            !unsat(&cs),
            "unsat must be conservative (never a spurious contradiction)"
        );
    }
}

// ===========================================================================
// Property-based tests (proptest)
// ===========================================================================

#[cfg(test)]
mod proptests {
    use proptest::prelude::*;
    use tpt_telos_ir::{Constraint, Linear, Relation};

    fn arb_relation() -> impl Strategy<Value = Relation> {
        prop_oneof![
            Just(Relation::Le),
            Just(Relation::Lt),
            Just(Relation::Ge),
            Just(Relation::Gt),
            Just(Relation::Eq),
        ]
    }

    /// Generate a simple linear expression with 0-2 variable terms.
    fn arb_linear() -> impl Strategy<Value = Linear> {
        (0..=2usize, -10i64..10i64).prop_flat_map(|(n, constant)| {
            prop::collection::vec(("var_[a-z]", -10i64..10i64), 0..=n).prop_map(move |terms| {
                let mut result_terms: Vec<(String, i64)> = Vec::new();
                for (v, c) in terms {
                    if c == 0 {
                        continue;
                    }
                    if let Some(existing) = result_terms.iter_mut().find(|(vv, _)| vv == &v) {
                        existing.1 += c;
                    } else {
                        result_terms.push((v, c));
                    }
                }
                result_terms.retain(|(_, c)| *c != 0);
                Linear {
                    terms: result_terms,
                    constant,
                }
            })
        })
    }

    fn arb_constraint() -> impl Strategy<Value = Constraint> {
        (arb_linear(), arb_relation()).prop_map(|(lin, rel)| Constraint(lin, rel))
    }

    fn arb_constraint_set() -> impl Strategy<Value = Vec<Constraint>> {
        prop::collection::vec(arb_constraint(), 1..=3)
    }

    proptest! {
        #[test]
        fn entails_refl(c in arb_constraint()) {
            // A constraint always entails itself.
            prop_assert!(super::entails(std::slice::from_ref(&c), &c));
        }

        #[test]
        fn unsat_contradiction(lo in -10i64..0i64, hi in 1i64..10i64) {
            // x >= hi && x <= lo is unsatisfiable when lo < hi.
            let cs = vec![
                Constraint(Linear::var("x").sub(&Linear::constant_only(hi)), Relation::Ge),
                Constraint(Linear::var("x").sub(&Linear::constant_only(lo)), Relation::Le),
            ];
            prop_assert!(super::unsat(&cs));
        }

        #[test]
        fn model_satisfies_its_constraints(cs in arb_constraint_set()) {
            if let Some(m) = super::model(&cs) {
                prop_assert!(super::satisfies_model(&cs, &m), "model {m:?} invalid");
            }
        }

        #[test]
        fn entails_implies_no_counterexample(premises in arb_constraint_set(), concl in arb_constraint()) {
            if super::entails(&premises, &concl) {
                prop_assert!(super::counterexample(&premises, &concl).is_none());
            }
        }

        #[test]
        fn counterexample_satisfies_negated(premises in arb_constraint_set(), concl in arb_constraint()) {
            if let Some(ce) = super::counterexample(&premises, &concl) {
                // The counter-example should satisfy premises ∧ ¬conclusion.
                // The negation produces branches (disjunction); the CE satisfies
                // at least one branch (the one that made it sat).
                let mut any_branch_satisfied = false;
                for branch in super::negate(&concl) {
                    let mut combined = premises.clone();
                    combined.extend(branch);
                    if super::satisfies_model(&combined, &ce) {
                        any_branch_satisfied = true;
                        break;
                    }
                }
                prop_assert!(any_branch_satisfied, "CE {ce:?} invalid for no branch");
            }
        }

        #[test]
        fn negation_roundtrip(rel in arb_relation(), coeff in -5i64..5i64, k in -5i64..5i64) {
            let concl = Constraint(
                Linear { terms: vec![("x".to_string(), coeff)], constant: k },
                rel,
            );
            let empty: Vec<Constraint> = vec![];
            let entailed = super::entails(&empty, &concl);
            let mut neg_unsat = true;
            for branch in super::negate(&concl) {
                let mut combined = vec![];
                combined.extend(branch);
                if !super::unsat(&combined) {
                    neg_unsat = false;
                    break;
                }
            }
            prop_assert_eq!(entailed, neg_unsat);
        }
    }
}
