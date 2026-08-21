//! Symbolic byte-size arithmetic for allocation proofs.
//!
//! An allocation's size in bytes is a function of its tensor's dimensions. Some
//! dimensions are symbolic (named variables that tpt-telos maps to Z3/FM integer
//! variables); others are fixed compile-time constants. [`SizeExpr`] captures
//! that arithmetic so the prover can either linearize it for the built-in
//! Fourier-Motzkin engine or translate it directly to a Z3 integer expression.

use std::collections::BTreeMap;

/// An integer arithmetic expression over (possibly symbolic) dimension names.
#[derive(Debug, Clone, PartialEq)]
pub enum SizeExpr {
    /// A fixed constant (e.g. a `Dimension::Fixed` size or a dtype element width).
    Const(i64),
    /// A symbolic dimension name (`Dimension::Symbolic` / `Dimension::Bounded`).
    Var(String),
    Add(Box<SizeExpr>, Box<SizeExpr>),
    Mul(Box<SizeExpr>, Box<SizeExpr>),
    /// Integer scalar multiplication (`Scale(k, e)` == `k * e`).
    Scale(i64, Box<SizeExpr>),
}

/// Error returned when a [`SizeExpr`] cannot be linearized because it contains a
/// product of two non-constant factors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinearizeError {
    Nonlinear,
}

impl SizeExpr {
    pub fn const_(n: i64) -> Self {
        SizeExpr::Const(n)
    }

    pub fn var(name: impl Into<String>) -> Self {
        SizeExpr::Var(name.into())
    }

    /// True iff the expression is *linear* in its variables — i.e. it contains no
    /// product of two non-constant factors. The built-in Fourier-Motzkin engine
    /// can only decide linear arithmetic; nonlinear sizes require the `z3` feature.
    pub fn is_linear(&self) -> bool {
        match self {
            SizeExpr::Const(_) | SizeExpr::Var(_) => true,
            SizeExpr::Scale(_, e) => e.is_linear(),
            SizeExpr::Add(a, b) => a.is_linear() && b.is_linear(),
            SizeExpr::Mul(a, b) => match (a.as_ref(), b.as_ref()) {
                (SizeExpr::Const(_), _) | (_, SizeExpr::Const(_)) => a.is_linear() && b.is_linear(),
                _ => false,
            },
        }
    }

    /// Linearize into `(coefficients, constant)` such that the expression equals
    /// `sum(coeff * var) + constant`.
    ///
    /// # Panics
    ///
    /// Panics if the expression is not linear. Callers must check
    /// [`SizeExpr::is_linear`] first (or use [`SizeExpr::try_linearize`]).
    pub fn linearize(&self) -> (BTreeMap<String, i64>, i64) {
        self.try_linearize()
            .expect("SizeExpr::linearize called on a nonlinear expression")
    }

    /// Like [`SizeExpr::linearize`] but returns `Err` instead of panicking when
    /// the expression is nonlinear.
    pub fn try_linearize(&self) -> Result<(BTreeMap<String, i64>, i64), LinearizeError> {
        match self {
            SizeExpr::Const(n) => Ok((BTreeMap::new(), *n)),
            SizeExpr::Var(v) => {
                let mut m = BTreeMap::new();
                m.insert(v.clone(), 1);
                Ok((m, 0))
            }
            SizeExpr::Add(a, b) => {
                let (m1, c1) = a.try_linearize()?;
                let (m2, c2) = b.try_linearize()?;
                Ok((merge_maps(m1, m2), c1 + c2))
            }
            SizeExpr::Scale(k, e) => {
                let (m, c) = e.try_linearize()?;
                Ok((m.into_iter().map(|(v, co)| (v, co * k)).collect(), c * k))
            }
            SizeExpr::Mul(a, b) => {
                let (m1, c1) = a.try_linearize()?;
                let (m2, c2) = b.try_linearize()?;
                // Linear only when at least one factor is a pure constant.
                if m1.is_empty() {
                    Ok((
                        m2.into_iter().map(|(v, co)| (v, co * c1)).collect(),
                        c1 * c2,
                    ))
                } else if m2.is_empty() {
                    Ok((
                        m1.into_iter().map(|(v, co)| (v, co * c2)).collect(),
                        c1 * c2,
                    ))
                } else {
                    Err(LinearizeError::Nonlinear)
                }
            }
        }
    }

    /// Evaluate the expression against a concrete integer model.
    pub fn eval(&self, model: &std::collections::HashMap<String, i64>) -> i64 {
        match self {
            SizeExpr::Const(n) => *n,
            SizeExpr::Var(v) => model.get(v).copied().unwrap_or(0),
            SizeExpr::Add(a, b) => a.eval(model) + b.eval(model),
            SizeExpr::Mul(a, b) => a.eval(model) * b.eval(model),
            SizeExpr::Scale(k, e) => k * e.eval(model),
        }
    }

    /// Build a Z3 integer AST for this expression (only available with the `z3`
    /// feature and when Z3 is present at runtime).
    #[cfg(feature = "z3")]
    pub fn to_z3<'a>(&self, ctx: &'a z3::Context) -> z3::ast::Int<'a> {
        match self {
            SizeExpr::Const(n) => z3::ast::Int::from_i64(ctx, *n),
            SizeExpr::Var(v) => z3::ast::Int::new_const(ctx, v.as_str()),
            SizeExpr::Add(a, b) => a.to_z3(ctx) + b.to_z3(ctx),
            SizeExpr::Mul(a, b) => a.to_z3(ctx) * b.to_z3(ctx),
            SizeExpr::Scale(k, e) => z3::ast::Int::from_i64(ctx, *k) * e.to_z3(ctx),
        }
    }
}

fn merge_maps(a: BTreeMap<String, i64>, b: BTreeMap<String, i64>) -> BTreeMap<String, i64> {
    let mut out = a;
    for (v, c) in b {
        *out.entry(v).or_insert(0) += c;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_detection() {
        let linear = SizeExpr::Scale(
            4,
            Box::new(SizeExpr::Mul(
                Box::new(SizeExpr::Var("n".to_string())),
                Box::new(SizeExpr::Const(8)),
            )),
        );
        assert!(linear.is_linear());
        let (coeffs, c) = linear.linearize();
        assert_eq!(coeffs.get("n").copied(), Some(32));
        assert_eq!(c, 0);

        let nonlinear = SizeExpr::Mul(
            Box::new(SizeExpr::Var("a".to_string())),
            Box::new(SizeExpr::Var("b".to_string())),
        );
        assert!(!nonlinear.is_linear());
        assert!(nonlinear.try_linearize().is_err());
    }

    #[test]
    fn eval_matches_linearize() {
        let e = SizeExpr::Add(
            Box::new(SizeExpr::Scale(2, Box::new(SizeExpr::Var("x".to_string())))),
            Box::new(SizeExpr::Const(5)),
        );
        let mut m = std::collections::HashMap::new();
        m.insert("x".to_string(), 10);
        assert_eq!(e.eval(&m), 25);
    }
}
