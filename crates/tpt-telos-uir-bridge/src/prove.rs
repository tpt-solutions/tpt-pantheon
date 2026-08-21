//! The memory-bound proof itself.
//!
//! For every memory `scope` in the region, the prover sums the symbolic byte
//! sizes of the `tpt_memory.alloc` operations that belong to it and asks whether
//! that sum can ever exceed the scope's hardware limit. "Never exceeds" is proven
//! by showing its negation — `total >= limit + 1` — is *unsatisfiable*.
//!
//! The negation being *satisfiable* yields a concrete witness (a binding of the
//! symbolic dimensions) under which the scope overflows its budget; that witness
//! is surfaced as [`ProofResult::Counterexample`].

use std::collections::{BTreeMap, HashMap};

use tpt_telos_ir::{Constraint, Linear, Relation};
use tpt_telos_verifier::model as fm_model;

use tpt_uir_core::ir::Region;

use crate::expr::SizeExpr;
use crate::extract::{extract_allocs, extract_bounded_dims, extract_symbolic_dims};

/// The outcome of a memory-bound proof over a TPT-UIR region.
#[derive(Debug, Clone, PartialEq)]
pub enum ProofResult {
    /// Every memory scope's allocation total is provably within its budget for
    /// all assignments to the symbolic dimensions.
    Valid,
    /// At least one scope can exceed its budget. `model` is a concrete witness
    /// (a binding of the symbolic dimension variables) achieving the overflow.
    Counterexample {
        scope: String,
        model: HashMap<String, i64>,
        total_bytes: i64,
        limit_bytes: i64,
    },
    /// The proof could not be decided by the current engine (e.g. a nonlinear
    /// allocation size evaluated without the `z3` feature). The reason explains
    /// what is needed to decide it.
    Inconclusive { reason: String },
}

/// Per-scope and default physical-memory budgets, in bytes.
#[derive(Debug, Clone)]
pub struct MemoryLimits {
    /// Budget used for any scope not present in [`MemoryLimits::per_scope`].
    pub default: i64,
    /// Explicit per-scope budgets keyed by the `mem.scope_begin` `lifetime`.
    pub per_scope: HashMap<String, i64>,
}

impl MemoryLimits {
    /// A budget that treats every scope as effectively unbounded (`i64::MAX`).
    /// Use only for smoke tests; real proofs must set finite limits.
    pub fn unbounded() -> Self {
        MemoryLimits {
            default: i64::MAX,
            per_scope: HashMap::new(),
        }
    }

    /// A single default budget applied to every scope.
    pub fn with_default(bytes: i64) -> Self {
        MemoryLimits {
            default: bytes,
            per_scope: HashMap::new(),
        }
    }

    /// Set (or override) the budget for one named scope.
    pub fn with_scope(mut self, name: impl Into<String>, bytes: i64) -> Self {
        self.per_scope.insert(name.into(), bytes);
        self
    }
}

impl Default for MemoryLimits {
    fn default() -> Self {
        MemoryLimits::unbounded()
    }
}

/// Prove that every memory scope in `region` stays within its budget.
pub fn prove_memory_bounds(region: &Region, limits: &MemoryLimits) -> ProofResult {
    let allocs = extract_allocs(region);
    let symbolic = extract_symbolic_dims(region);
    let bounded = extract_bounded_dims(region);

    // Group allocation byte-sizes by scope name.
    let mut by_scope: BTreeMap<String, Vec<SizeExpr>> = BTreeMap::new();
    for a in &allocs {
        by_scope
            .entry(a.scope.clone())
            .or_default()
            .push(a.byte_size.clone());
    }

    let global_bounds = build_global_constraints(&symbolic, &bounded);

    for (scope, exprs) in &by_scope {
        let limit = limits
            .per_scope
            .get(scope)
            .copied()
            .unwrap_or(limits.default);
        if limit == i64::MAX {
            continue; // explicitly unbounded scope
        }

        let mut total = SizeExpr::const_(0);
        for e in exprs {
            total = SizeExpr::Add(Box::new(total), Box::new(e.clone()));
        }

        if total.is_linear() {
            if let Some(ce) = decide_scope_linear(&total, limit, &global_bounds) {
                let total_bytes = total.eval(&ce);
                return ProofResult::Counterexample {
                    scope: scope.clone(),
                    model: ce,
                    total_bytes,
                    limit_bytes: limit,
                };
            }
        } else {
            // Nonlinear allocation size.
            #[cfg(feature = "z3")]
            {
                if tpt_telos_verifier::z3_solver::is_z3_available() {
                    if let Some(ce) = solve_z3(&total, limit, &global_bounds) {
                        let total_bytes = total.eval(&ce);
                        return ProofResult::Counterexample {
                            scope: scope.clone(),
                            model: ce,
                            total_bytes,
                            limit_bytes: limit,
                        };
                    }
                    // Z3 says the over-budget branch is unsat -> scope is safe.
                    continue;
                }
            }
            // No exact solver available for the nonlinear size.
            return ProofResult::Inconclusive {
                reason: format!(
                    "scope '{scope}' has a nonlinear allocation size (product of symbolic \
                     dimensions); enable the `z3` feature for exact solving"
                ),
            };
        }
    }

    ProofResult::Valid
}

/// Returns `Some(model)` if a witness exists making the (linear) scope overflow,
/// or `None` if the scope is provably within budget.
fn decide_scope_linear(
    total: &SizeExpr,
    limit: i64,
    global_bounds: &[Constraint],
) -> Option<HashMap<String, i64>> {
    let (coeffs, constant) = total.linearize();
    let mut lin = Linear {
        terms: coeffs.into_iter().collect(),
        constant,
    };
    // over-budget branch: total >= limit + 1  <=>  total - (limit + 1) >= 0
    lin.constant -= limit + 1;
    let mut cs: Vec<Constraint> = global_bounds.to_vec();
    cs.push(Constraint(lin, Relation::Ge));
    // `fm_model` returns Some only when the over-budget branch is satisfiable,
    // i.e. an overflow witness exists.
    fm_model(&cs)
}

/// Shared non-negativity / bounded-max constraints over every symbolic dimension.
fn build_global_constraints(
    symbolic: &std::collections::BTreeSet<String>,
    bounded: &[(String, usize)],
) -> Vec<Constraint> {
    let mut cs = Vec::new();
    for v in symbolic {
        cs.push(Constraint(Linear::var(v), Relation::Ge)); // v >= 0
    }
    for (s, max_value) in bounded {
        cs.push(Constraint(Linear::var(s), Relation::Ge)); // s >= 0
        let le = Linear::var(s).sub(&Linear::constant_only(*max_value as i64));
        cs.push(Constraint(le, Relation::Le)); // s <= max_value
    }
    cs
}

/// Prove a region that has been serialized as `.tptuir` postcard bytes.
pub fn prove_tptuir_bytes(
    bytes: &[u8],
    limits: &MemoryLimits,
) -> Result<ProofResult, postcard::Error> {
    let region = tpt_uir_serde::deserialize_region(bytes)?;
    Ok(prove_memory_bounds(&region, limits))
}

#[cfg(feature = "z3")]
fn solve_z3(
    total: &SizeExpr,
    limit: i64,
    global_bounds: &[Constraint],
) -> Option<HashMap<String, i64>> {
    use z3::{ast::Int, Config, Context, SatResult, Solver};

    let cfg = Config::new();
    let ctx = Context::new(&cfg);
    let solver = Solver::new(&ctx);
    let zero = Int::from_i64(&ctx, 0);

    for c in global_bounds {
        let ast = z3_constraint(&ctx, c);
        match c.1 {
            Relation::Le => solver.assert(&(ast <= zero.clone())),
            Relation::Lt => solver.assert(&(ast < zero.clone())),
            Relation::Ge => solver.assert(&(ast >= zero.clone())),
            Relation::Gt => solver.assert(&(ast > zero.clone())),
            Relation::Eq => solver.assert(&ast._eq(&zero.clone())),
            Relation::Ne => solver.assert(&ast._eq(&zero.clone()).not()),
        }
    }

    // over-budget branch: total >= limit + 1
    let tot = total.to_z3(&ctx);
    let rhs = Int::from_i64(&ctx, limit + 1);
    solver.assert(&(tot >= rhs));

    if solver.check() != SatResult::Sat {
        return None;
    }

    let m = solver.get_model()?;
    let mut names = collect_var_names(total, global_bounds);
    let mut out = HashMap::new();
    for v in &mut names {
        let va = Int::new_const(&ctx, v.as_str());
        if let Some(val) = m.eval(&va, true) {
            if let Some(i) = val.as_i64() {
                out.insert(v.clone(), i);
            }
        }
    }
    Some(out)
}

#[cfg(feature = "z3")]
fn z3_constraint<'a>(ctx: &'a Context, c: &Constraint) -> Int<'a> {
    let Constraint(lin, _rel) = c;
    let mut expr = Int::from_i64(ctx, lin.constant);
    for (var, coeff) in &lin.terms {
        let var_ast = Int::new_const(ctx, var.as_str());
        expr = expr + var_ast * *coeff;
    }
    expr
}

#[cfg(feature = "z3")]
fn collect_var_names(total: &SizeExpr, global_bounds: &[Constraint]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    fn walk(e: &SizeExpr, names: &mut Vec<String>) {
        match e {
            SizeExpr::Const(_) => {}
            SizeExpr::Var(v) => {
                if !names.contains(v) {
                    names.push(v.clone());
                }
            }
            SizeExpr::Add(a, b) | SizeExpr::Mul(a, b) => {
                walk(a, names);
                walk(b, names);
            }
            SizeExpr::Scale(_, e) => walk(e, names),
        }
    }
    walk(total, &mut names);
    for c in global_bounds {
        for (v, _) in &c.0.terms {
            if !names.contains(v) {
                names.push(v.clone());
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_uir_core::attr::{Attribute, AttributeValue};
    use tpt_uir_core::op_name::OpName;
    use tpt_uir_core::types::{Dimension, ScalarType, ShapeSpec, TensorType, Type};
    use tpt_uir_core::{Block, Operation, Region, ValueId};

    fn tensor_op(id: ValueId, dtype: ScalarType, dims: Vec<Dimension>, scope: &str) -> Operation {
        Operation {
            id,
            op_name: OpName::new("tpt_memory", "alloc"),
            operands: vec![id],
            results: vec![],
            regions: vec![],
            attributes: vec![
                Attribute::string("scope", scope.to_string()),
                Attribute {
                    key: "tensor".to_string(),
                    value: AttributeValue::Type(Type::Tensor(TensorType {
                        dtype,
                        shape: Some(ShapeSpec { dimensions: dims }),
                    })),
                },
            ],
        }
    }

    fn region_with(ops: Vec<Operation>) -> Region {
        Region {
            blocks: vec![Block {
                arguments: vec![],
                operations: ops,
            }],
        }
    }

    #[test]
    fn all_fixed_valid() {
        // Two allocations of 1024 and 2048 bytes, limit 4096 -> within budget.
        let ops = vec![
            tensor_op(1, ScalarType::F32, vec![Dimension::Fixed(256)], "s0"),
            tensor_op(2, ScalarType::F32, vec![Dimension::Fixed(512)], "s0"),
        ];
        let region = region_with(ops);
        let res = prove_memory_bounds(&region, &MemoryLimits::with_default(4096));
        assert_eq!(res, ProofResult::Valid);
    }

    #[test]
    fn all_fixed_over_budget() {
        let ops = vec![
            tensor_op(1, ScalarType::F32, vec![Dimension::Fixed(256)], "s0"), // 1024
            tensor_op(2, ScalarType::F32, vec![Dimension::Fixed(512)], "s0"), // 2048
        ];
        let region = region_with(ops);
        let res = prove_memory_bounds(&region, &MemoryLimits::with_default(3000));
        match res {
            ProofResult::Counterexample {
                scope,
                total_bytes,
                limit_bytes,
                ..
            } => {
                assert_eq!(scope, "s0");
                assert_eq!(total_bytes, 3072);
                assert_eq!(limit_bytes, 3000);
            }
            other => panic!("expected Counterexample, got {other:?}"),
        }
    }

    #[test]
    fn symbolic_unconstrained_can_overflow() {
        // One symbolic dim n, elem F32 (4 bytes) -> 4*n bytes, n >= 0 unconstrained.
        // 4n can exceed any finite limit, so a witness exists.
        let ops = vec![tensor_op(
            1,
            ScalarType::F32,
            vec![Dimension::Symbolic("n".to_string())],
            "s0",
        )];
        let region = region_with(ops);
        let res = prove_memory_bounds(&region, &MemoryLimits::with_default(4000));
        match res {
            ProofResult::Counterexample { model, .. } => {
                // witness should have n large enough that 4n >= 4001.
                assert!(model["n"] * 4 >= 4001);
            }
            other => panic!("expected Counterexample, got {other:?}"),
        }
    }

    #[test]
    fn symbolic_inconclusive_without_z3_when_nonlinear() {
        // Two symbolic dims -> nonlinear (n*m) byte size. Without Z3, inconclusive.
        let ops = vec![tensor_op(
            1,
            ScalarType::F32,
            vec![
                Dimension::Symbolic("n".to_string()),
                Dimension::Symbolic("m".to_string()),
            ],
            "s0",
        )];
        let region = region_with(ops);
        let res = prove_memory_bounds(&region, &MemoryLimits::with_default(1000));
        match res {
            ProofResult::Inconclusive { .. } => {}
            other => panic!("expected Inconclusive without z3, got {other:?}"),
        }
    }

    #[test]
    fn bounded_dim_respects_max() {
        // n bounded by 10, elem F32 (4 bytes) -> 4n <= 40 always. Limit 100 -> Valid.
        let ops = vec![tensor_op(
            1,
            ScalarType::F32,
            vec![Dimension::Bounded {
                symbol: "n".to_string(),
                max_value: 10,
            }],
            "s0",
        )];
        let region = region_with(ops);
        let res = prove_memory_bounds(&region, &MemoryLimits::with_default(100));
        assert_eq!(res, ProofResult::Valid);
    }

    #[test]
    fn separate_scopes_independent() {
        let ops = vec![
            tensor_op(1, ScalarType::F32, vec![Dimension::Fixed(256)], "fast"), // 1024
            tensor_op(2, ScalarType::F32, vec![Dimension::Fixed(512)], "slow"), // 2048
        ];
        let region = region_with(ops);
        let limits = MemoryLimits::unbounded()
            .with_scope("fast", 2048)
            .with_scope("slow", 1024); // slow over budget
        match prove_memory_bounds(&region, &limits) {
            ProofResult::Counterexample {
                scope, total_bytes, ..
            } => {
                assert_eq!(scope, "slow");
                assert_eq!(total_bytes, 2048);
            }
            other => panic!("expected Counterexample, got {other:?}"),
        }
    }
}
