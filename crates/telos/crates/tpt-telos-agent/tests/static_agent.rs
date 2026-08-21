//! Unit tests for the agentic transpiler core (tpt-telos Phase 2).
//!
//! Focuses on the [`StaticAgent`] synthesis logic in isolation (not just the
//! end-to-end `transpile` pipeline) and on the Generate -> Verify ->
//! Counter-example -> Rewrite loop hitting its retry/failure limit.

use std::cell::Cell;
use std::collections::HashMap;

use tpt_telos_agent::static_agent::synthesize_from_ensures;
use tpt_telos_agent::{transpile_func, Candidate, CodeAgent, FuncSpec, StaticAgent};
use tpt_telos_parser::ast::*;
use tpt_telos_parser::Span;

// ---- AST builders ---------------------------------------------------------

fn var(v: &str) -> Expr {
    Expr::Var(v.to_string())
}
fn int(n: i64) -> Expr {
    Expr::Int(n)
}
fn field(base: &str, f: &str) -> Expr {
    Expr::Field {
        base: base.to_string(),
        field: f.to_string(),
    }
}
fn old(e: Expr) -> Expr {
    Expr::Old(Box::new(e))
}
fn bin(op: BinOp, l: Expr, r: Expr) -> Expr {
    Expr::Bin {
        op,
        lhs: Box::new(l),
        rhs: Box::new(r),
    }
}
fn assign(target: Expr, op: AssignOp, value: Expr) -> Assign {
    Assign { target, op, value }
}

fn func_with(
    name: &str,
    params: Vec<Param>,
    requires: Vec<Expr>,
    ensures: Vec<Expr>,
    body: Vec<Stmt>,
    elided: bool,
) -> Func {
    Func {
        attributes: vec![],
        name: name.to_string(),
        params,
        return_ty: None,
        requires,
        ensures,
        body,
        elided,
        span: Span::default(),
        requires_spans: vec![],
        ensures_spans: vec![],
    }
}

// ---- synthesize_from_ensures ---------------------------------------------

#[test]
fn synthesize_field_and_scalar_from_ensures() {
    // ensures c.v == old(c.v) + by   ;   ensures out == c.v
    let func = func_with(
        "f",
        vec![
            Param {
                name: "c".into(),
                ty: Type::Named("Counter".into()),
                mutability: ParamMutability::Immutable,
            },
            Param {
                name: "by".into(),
                ty: Type::Named("PositiveInt".into()),
                mutability: ParamMutability::Immutable,
            },
        ],
        vec![],
        vec![
            bin(
                BinOp::Eq,
                field("c", "v"),
                bin(BinOp::Add, old(field("c", "v")), var("by")),
            ),
            bin(BinOp::Eq, var("out"), field("c", "v")),
        ],
        vec![],
        true,
    );
    let spec = FuncSpec::new(vec![], func);
    let cand = synthesize_from_ensures(&spec);

    let text = tpt_telos_agent::render_candidate(&cand);
    // `old(...)` must be resolved away in the synthesized body.
    assert!(!text.contains("old("), "old() should be resolved: {text}");
    // Field assignment uses the current state, not the pre-state.
    assert!(
        text.contains("c.v = c.v + by"),
        "missing field assign: {text}"
    );
    // Scalar output is bound as a return value.
    assert!(text.contains("out = c.v"), "missing scalar assign: {text}");
}

#[test]
fn synthesize_resolves_nested_old_arithmetic() {
    // ensures c.v == old(c.v) * 2  =>  c.v = c.v * 2
    let func = func_with(
        "f",
        vec![Param {
            name: "c".into(),
            ty: Type::Named("Counter".into()),
            mutability: ParamMutability::Immutable,
        }],
        vec![],
        vec![bin(
            BinOp::Eq,
            field("c", "v"),
            bin(BinOp::Mul, old(field("c", "v")), int(2)),
        )],
        vec![],
        true,
    );
    let spec = FuncSpec::new(vec![], func);
    let cand = synthesize_from_ensures(&spec);
    let text = tpt_telos_agent::render_candidate(&cand);
    assert!(text.contains("c.v = c.v * 2"), "missing mul assign: {text}");
}

#[test]
fn synthesize_inequality_clause_produces_conservative_zero() {
    // A `>=` clause (e.g. `ensures x >= 0`) should synthesise `x = 0` as a
    // conservative first attempt rather than silently producing an empty body.
    let func = func_with(
        "f",
        vec![Param {
            name: "x".into(),
            ty: Type::Named("i64".into()),
            mutability: ParamMutability::Immutable,
        }],
        vec![],
        vec![bin(BinOp::Ge, var("x"), int(0))],
        vec![],
        true,
    );
    let spec = FuncSpec::new(vec![], func);
    let cand = synthesize_from_ensures(&spec);
    assert!(
        !cand.stmts.is_empty(),
        "inequality ensures must synthesize a conservative body, not an empty one"
    );
    // The synthesised body must assign 0 to `x`.
    assert!(
        matches!(
            &cand.stmts[0],
            Stmt::Assign(a)
                if matches!(&a.target, Expr::Var(v) if v == "x")
                    && matches!(&a.value, Expr::Int(0))
        ),
        "expected assignment `x = 0`, got {:?}",
        cand.stmts
    );
}

// ---- StaticAgent::generate / rewrite -------------------------------------

#[test]
fn generate_returns_developer_body_when_present() {
    let body = vec![Stmt::MutateState(vec![assign(
        field("c", "v"),
        AssignOp::Set,
        bin(BinOp::Add, field("c", "v"), int(1)),
    )])];
    let func = func_with(
        "f",
        vec![Param {
            name: "c".into(),
            ty: Type::Named("Counter".into()),
            mutability: ParamMutability::Immutable,
        }],
        vec![],
        vec![],
        body.clone(),
        false,
    );
    let spec = FuncSpec::new(vec![], func);
    let agent = StaticAgent::new();
    let cand = agent.generate(&spec).unwrap();
    assert_eq!(cand.stmts, body);
}

#[test]
fn generate_synthesizes_when_elided() {
    let func = func_with(
        "f",
        vec![Param {
            name: "c".into(),
            ty: Type::Named("Counter".into()),
            mutability: ParamMutability::Immutable,
        }],
        vec![],
        vec![bin(
            BinOp::Eq,
            field("c", "v"),
            bin(BinOp::Add, old(field("c", "v")), int(1)),
        )],
        vec![],
        true,
    );
    let spec = FuncSpec::new(vec![], func);
    let agent = StaticAgent::new();
    let cand = agent.generate(&spec).unwrap();
    assert!(!cand.stmts.is_empty());
}

#[test]
fn transpile_scalar_out_ensures_verifies() {
    // Regression: an elided function with a scalar `ensures out == a + b`
    // clause produces a bare `out = a + b;` local assignment. The IR extractor
    // must accept a bare `Expr::Var` assignment target via `assign_constraint`
    // and `problem_for` must surface any internal extraction failure as a
    // normal `Err` (never a panic).
    let func = func_with(
        "out",
        vec![
            Param {
                name: "a".into(),
                ty: Type::Named("Int".into()),
                mutability: ParamMutability::Immutable,
            },
            Param {
                name: "b".into(),
                ty: Type::Named("Int".into()),
                mutability: ParamMutability::Immutable,
            },
        ],
        vec![
            bin(BinOp::Ge, var("a"), int(0)),
            bin(BinOp::Ge, var("b"), int(0)),
        ],
        vec![bin(
            BinOp::Eq,
            var("out"),
            bin(BinOp::Add, var("a"), var("b")),
        )],
        vec![],
        true,
    );
    let module = Module {
        attributes: vec![],
        name: "M".to_string(),
        items: vec![Item::Func(func)],
    };
    let outcome = transpile_func(&module, 0, &StaticAgent::new()).unwrap();
    assert!(
        outcome.verified,
        "scalar out should verify: {:?}",
        outcome.result
    );
}

#[test]
fn rewrite_fixes_broken_field_from_counter_example() {
    // Broken body: `from.balance += amount` (should be `-=`).
    let broken = Candidate {
        stmts: vec![Stmt::MutateState(vec![assign(
            field("from", "balance"),
            AssignOp::Add,
            var("amount"),
        )])],
    };
    let func = func_with(
        "transfer",
        vec![
            Param {
                name: "from".into(),
                ty: Type::Named("Wallet".into()),
                mutability: ParamMutability::Immutable,
            },
            Param {
                name: "amount".into(),
                ty: Type::Named("i64".into()),
                mutability: ParamMutability::Immutable,
            },
        ],
        vec![],
        vec![bin(
            BinOp::Eq,
            field("from", "balance"),
            bin(BinOp::Sub, old(field("from", "balance")), var("amount")),
        )],
        vec![],
        true,
    );
    let spec = FuncSpec::new(vec![], func);

    // Counter-example: pre from.balance=10, amount=3, but the wrong body yields
    // from.balance' = 13 (the `+` result) instead of the required 7.
    let ce: HashMap<String, i64> = HashMap::from([
        ("from.balance".to_string(), 10),
        ("amount".to_string(), 3),
        ("from.balance'".to_string(), 13),
    ]);

    let agent = StaticAgent::new();
    let repaired = agent.rewrite(&spec, &broken, &ce).unwrap();
    let text = tpt_telos_agent::render_candidate(&repaired);
    assert!(
        text.contains("from.balance - amount"),
        "repair should subtract: {text}"
    );
    assert!(
        !text.contains("from.balance + amount"),
        "repair must not add: {text}"
    );
}

#[test]
fn rewrite_with_empty_counter_example_resynthesizes() {
    let broken = Candidate {
        stmts: vec![Stmt::MutateState(vec![assign(
            field("c", "v"),
            AssignOp::Set,
            int(0),
        )])],
    };
    let func = func_with(
        "f",
        vec![Param {
            name: "c".into(),
            ty: Type::Named("Counter".into()),
            mutability: ParamMutability::Immutable,
        }],
        vec![],
        vec![bin(
            BinOp::Eq,
            field("c", "v"),
            bin(BinOp::Add, old(field("c", "v")), int(1)),
        )],
        vec![],
        true,
    );
    let spec = FuncSpec::new(vec![], func);
    let agent = StaticAgent::new();
    let repaired = agent.rewrite(&spec, &broken, &HashMap::new()).unwrap();
    let text = tpt_telos_agent::render_candidate(&repaired);
    assert!(
        text.contains("c.v = c.v + 1"),
        "should resynthesize from contract: {text}"
    );
}

// ---- loop retry / failure limit ------------------------------------------

/// A deliberately unhelpful agent: it returns an empty body on `generate` and,
/// on every `rewrite`, appends a fresh dummy field assignment so the candidate
/// keeps changing (never triggering the static fallback) but never satisfies
/// the contract. This forces the Generate -> Verify -> Rewrite loop to run to
/// its `MAX_ITERS` retry limit and give up.
struct LoopKiller {
    n: Cell<usize>,
}

impl LoopKiller {
    fn new() -> Self {
        LoopKiller { n: Cell::new(0) }
    }
}

impl CodeAgent for LoopKiller {
    fn name(&self) -> &str {
        "loop-killer"
    }

    fn generate(&self, _spec: &FuncSpec) -> Result<Candidate, String> {
        Ok(Candidate { stmts: vec![] })
    }

    fn rewrite(
        &self,
        _spec: &FuncSpec,
        prev: &Candidate,
        _ce: &HashMap<String, i64>,
    ) -> Result<Candidate, String> {
        let k = self.n.get();
        self.n.set(k + 1);
        let mut stmts = prev.stmts.clone();
        stmts.push(Stmt::MutateState(vec![assign(
            field("s", &format!("dummy{k}")),
            AssignOp::Set,
            int(0),
        )]));
        Ok(Candidate { stmts })
    }
}

#[test]
fn loop_hits_retry_limit_and_gives_up() {
    // `ensures s.v == 0` on an elided function; the LoopKiller can never satisfy
    // it, so `transpile_func` must terminate at its limit without verifying.
    let func = func_with(
        "stuck",
        vec![Param {
            name: "s".into(),
            ty: Type::Named("S".into()),
            mutability: ParamMutability::Immutable,
        }],
        vec![],
        vec![bin(BinOp::Eq, field("s", "v"), int(0))],
        vec![],
        true,
    );
    let module = Module {
        attributes: vec![],
        name: "M".to_string(),
        items: vec![Item::Func(func)],
    };

    let outcome = transpile_func(&module, 0, &LoopKiller::new()).unwrap();
    // The loop must terminate and report failure...
    assert!(!outcome.verified, "LoopKiller agent must never verify");
    // ...after retrying (not on the very first step) but without exceeding the
    // configured retry limit.
    assert!(outcome.iterations.len() >= 2, "expected multiple retries");
    assert!(
        outcome.iterations.len() <= 8,
        "loop must respect its retry limit, got {}",
        outcome.iterations.len()
    );
}
