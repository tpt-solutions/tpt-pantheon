//! Intermediate representation for verification.
//!
//! All constraints are lowered into *linear* form: a sum of
//! `coefficient * variable` plus a constant, compared against zero.
//! This is the fragment handled by the QF_LRA solver in `telos-verifier`.

pub use tpt_telos_parser::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relation {
    Le, // <= 0
    Lt, // <  0
    Ge, // >= 0
    Gt, // >  0
    Eq, // == 0
    Ne, // != 0
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Linear {
    /// `(variable, coefficient)` pairs.
    pub terms: Vec<(String, i64)>,
    pub constant: i64,
}

/// A linear arithmetic constraint: `linear_expression relation 0`.
///
/// The left-hand side is a [`Linear`] expression; the right-hand side is
/// always zero. To encode `x >= 5`, build `x - 5 >= 0`:
///
/// ```
/// use tpt_telos_ir::{Constraint, Linear, Relation};
///
/// // x - 5 >= 0  encodes  x >= 5
/// let c = Constraint(
///     Linear::var("x").sub(&Linear::constant_only(5)),
///     Relation::Ge,
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constraint(pub Linear, pub Relation);

impl Linear {
    /// Create a [`Linear`] expression representing a single variable with coefficient 1.
    ///
    /// # Examples
    ///
    /// ```
    /// use tpt_telos_ir::Linear;
    ///
    /// let x = Linear::var("x");
    /// assert_eq!(x.terms, vec![("x".to_string(), 1)]);
    /// assert_eq!(x.constant, 0);
    /// ```
    pub fn var(name: &str) -> Linear {
        Linear {
            terms: vec![(name.to_string(), 1)],
            constant: 0,
        }
    }

    /// Create a [`Linear`] expression with no variable terms and a fixed constant.
    ///
    /// # Examples
    ///
    /// ```
    /// use tpt_telos_ir::Linear;
    ///
    /// let five = Linear::constant_only(5);
    /// assert!(five.terms.is_empty());
    /// assert_eq!(five.constant, 5);
    /// ```
    pub fn constant_only(c: i64) -> Linear {
        Linear {
            terms: vec![],
            constant: c,
        }
    }

    /// Add two linear expressions term-by-term, cancelling zero-coefficient terms.
    ///
    /// # Examples
    ///
    /// ```
    /// use tpt_telos_ir::Linear;
    ///
    /// // x + y
    /// let sum = Linear::var("x").add(&Linear::var("y"));
    /// assert_eq!(sum.terms.len(), 2);
    ///
    /// // x + (-x) = 0  (the x term cancels)
    /// let zero = Linear::var("x").add(&Linear::var("x").neg());
    /// assert!(zero.terms.is_empty());
    /// assert_eq!(zero.constant, 0);
    /// ```
    pub fn add(&self, other: &Linear) -> Linear {
        let mut terms = self.terms.clone();
        for (v, c) in &other.terms {
            if let Some(existing) = terms.iter_mut().find(|(vv, _)| vv == v) {
                existing.1 += *c;
            } else {
                terms.push((v.clone(), *c));
            }
        }
        terms.retain(|(_, c)| *c != 0);
        Linear {
            terms,
            constant: self.constant + other.constant,
        }
    }

    /// Subtract `other` from `self` term-by-term.
    ///
    /// # Examples
    ///
    /// ```
    /// use tpt_telos_ir::Linear;
    ///
    /// // Encode x - 5 (useful for "x >= 5" as Constraint(x.sub(5), Ge))
    /// let expr = Linear::var("x").sub(&Linear::constant_only(5));
    /// assert_eq!(expr.constant, -5);
    /// ```
    pub fn sub(&self, other: &Linear) -> Linear {
        let neg = Linear {
            terms: other.terms.iter().map(|(v, c)| (v.clone(), -*c)).collect(),
            constant: -other.constant,
        };
        self.add(&neg)
    }

    /// Negate every coefficient and the constant.
    ///
    /// # Examples
    ///
    /// ```
    /// use tpt_telos_ir::Linear;
    ///
    /// let x = Linear::var("x"); // x + 0
    /// let neg = x.neg();        // -x + 0
    /// assert_eq!(neg.terms, vec![("x".to_string(), -1)]);
    /// ```
    pub fn neg(&self) -> Linear {
        Linear {
            terms: self.terms.iter().map(|(v, c)| (v.clone(), -*c)).collect(),
            constant: -self.constant,
        }
    }

    /// Multiply every coefficient and the constant by `k`.
    ///
    /// # Examples
    ///
    /// ```
    /// use tpt_telos_ir::Linear;
    ///
    /// // 3x + 9
    /// let expr = Linear::var("x").add(&Linear::constant_only(3)).scale(3);
    /// assert_eq!(expr.constant, 9);
    /// ```
    pub fn scale(&self, k: i64) -> Linear {
        Linear {
            terms: self.terms.iter().map(|(v, c)| (v.clone(), c * k)).collect(),
            constant: self.constant * k,
        }
    }
}

/// A single function's verification problem.
#[derive(Debug, Clone)]
pub struct VerificationProblem {
    pub func_name: String,
    /// Source location of the function's `func` declaration; the fallback
    /// location for conclusions that cannot be tied to a more specific span.
    pub func_span: Span,
    /// Facts assumed/known: pre-conditions, type constraints, entry invariants,
    /// and `mutate state` assignments (which define post-state variables).
    pub premises: Vec<Constraint>,
    /// Properties that must be proven from the premises.
    pub conclusions: Vec<Conclusion>,
}

#[derive(Debug, Clone)]
pub struct Conclusion {
    /// Human-readable description, e.g. the source clause.
    pub description: String,
    pub constraint: Constraint,
    /// True for post-condition `ensures`, false for a maintained invariant.
    pub is_ensures: bool,
    /// True when interval bounding was used to linearize a nonlinear product.
    /// The proof is sound but conservative: the constraint was replaced with a
    /// worst-case constant derived from the variable bounds in the premises.
    pub is_approximation: bool,
    /// Source location of the clause this conclusion was derived from (the
    /// `ensures` clause, or the owning invariant's constraint).
    pub location: Span,
    /// Disjunction group: when `Some(n)`, this conclusion belongs to group `n`
    /// along with other conclusions sharing the same group id. At least one
    /// conclusion in the group must be entailed for the group to pass. When
    /// `None`, the conclusion must be entailed independently.
    pub or_group: Option<usize>,
}
