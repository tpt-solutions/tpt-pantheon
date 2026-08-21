//! The agentic transpiler core for tpt-telos (Phase 2).
//!
//! This crate implements the *Generate -> Verify -> Counter-example -> Rewrite*
//! loop described in the design document:
//!
//! 1. An LLM-style [`CodeAgent`] produces a candidate implementation (a body of
//!    `mutate state` / assignment statements) from a function's intent
//!    (`requires` / `ensures` and `@boundary` routing).
//! 2. The formal verifier checks the candidate against the mathematical
//!    contract.
//! 3. On failure, the solver extracts a concrete counter-example (a variable
//!    assignment where the contract is violated).
//! 4. The agent rewrites the candidate using that counter-example, and the loop
//!    repeats until the contract is provably satisfied.
//!
//! The default [`StaticAgent`] is a fully offline, deterministic synthesizer:
//! it translates the developer's stated body when present, and otherwise
//! derives an implementation directly from the `ensures` clauses. Real LLM
//! backends can be plugged in behind the [`CodeAgent`] trait (see
//! [`llm_agent`] behind the `llm` feature).

pub mod static_agent;

#[cfg(feature = "llm")]
pub mod llm_agent;

pub use static_agent::{synthesize_from_ensures, StaticAgent};

use tpt_telos_ir::{extract, VerificationProblem};
use tpt_telos_parser::ast::*;
use tpt_telos_verifier::{counterexample, verify, Model, VerificationResult};

/// An owned view of a single function's verification intent, detached from the
/// surrounding module so agents can be reasoned about in isolation.
///
/// # Examples
///
/// ```
/// use tpt_telos_parser::parse;
/// use tpt_telos_agent::FuncSpec;
///
/// let modules = parse("module M { func f(x: T) ; }").unwrap();
/// let m = &modules[0];
/// if let tpt_telos_parser::ast::Item::Func(func) = &m.items[0] {
///     let spec = FuncSpec::new(m.attributes.clone(), func.clone());
///     assert_eq!(spec.func.name, "f");
/// }
/// ```
#[derive(Clone)]
pub struct FuncSpec {
    pub module_attrs: Vec<Attribute>,
    pub func: Func,
}

impl FuncSpec {
    pub fn new(module_attrs: Vec<Attribute>, func: Func) -> Self {
        FuncSpec { module_attrs, func }
    }
}

/// A candidate implementation produced by an agent: the statements that form
/// the function body.
///
/// # Examples
///
/// ```
/// use tpt_telos_agent::Candidate;
///
/// let empty = Candidate { stmts: vec![] };
/// assert!(empty.stmts.is_empty());
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct Candidate {
    pub stmts: Vec<Stmt>,
}

/// A code-generation agent. Implementations may be deterministic (offline) or
/// backed by an external LLM; both speak the same structured `Stmt` language so
/// the output is guaranteed to lower into compilable Rust/Go.
pub trait CodeAgent {
    /// Human-readable agent name (for logs).
    fn name(&self) -> &str;

    /// Produce an initial candidate implementation for `spec`.
    fn generate(&self, spec: &FuncSpec) -> Result<Candidate, String>;

    /// Rewrite a previous candidate using a concrete counter-example where the
    /// contract failed. `ce` maps variable names (pre-state `"base.field"` and
    /// post-state `"base.field'"`) to integer values.
    fn rewrite(&self, spec: &FuncSpec, prev: &Candidate, ce: &Model) -> Result<Candidate, String>;
}

/// One iteration of the agentic loop, for transparent reporting.
#[derive(Clone)]
pub struct LoopStep {
    pub iteration: usize,
    pub action: String,
    pub candidate: Candidate,
    pub passed: bool,
    pub counterexample: Option<Model>,
}

/// The full outcome of transpiling one function through the agentic loop.
#[derive(Clone)]
pub struct FuncOutcome {
    pub func_name: String,
    pub target: tpt_telos_router::Target,
    pub agent: String,
    pub iterations: Vec<LoopStep>,
    pub final_candidate: Candidate,
    pub problem: VerificationProblem,
    pub result: VerificationResult,
    pub verified: bool,
}

const MAX_ITERS: usize = 8;

/// Build the verification problem for a candidate body by substituting it into
/// the module (preserving the module's invariants and routing metadata).
fn problem_for(
    module: &Module,
    func_idx: usize,
    stmts: &[Stmt],
) -> Result<VerificationProblem, String> {
    let mut module = module.clone();
    module.items[func_idx] = match &module.items[func_idx] {
        Item::Func(f) => {
            let mut f = f.clone();
            f.body = stmts.to_vec();
            f.elided = false;
            Item::Func(f)
        }
        other => other.clone(),
    };
    let problems = extract(&[module.clone()])?;
    problems
        .into_iter()
        .find(|p| p.func_name == module.items[func_idx].func_name())
        .ok_or_else(|| {
            format!(
                "internal error: no verification problem extracted for function '{}'",
                module.items[func_idx].func_name()
            )
        })
}

fn find_counterexample(
    problem: &VerificationProblem,
    result: &VerificationResult,
) -> Option<Model> {
    for (concl, check) in problem.conclusions.iter().zip(result.checks.iter()) {
        if !check.passed {
            if let Some(m) = counterexample(&problem.premises, &concl.constraint) {
                return Some(m);
            }
        }
    }
    None
}

/// Run the agentic transpilation loop for a single function.
///
/// # Examples
///
/// ```
/// use tpt_telos_parser::parse;
/// use tpt_telos_agent::{transpile_func, StaticAgent};
///
/// let src = r#"
///     module Bank {
///         invariant Wallet { balance >= 0 }
///         func deposit(w: Wallet, amount: PositiveInt)
///             ensures w.balance == old(w.balance) + amount
///         ;
///     }
/// "#;
///
/// let modules = parse(src).unwrap();
/// let agent = StaticAgent::new();
/// // index 0 is the invariant; index 1 is the `deposit` function
/// let outcome = transpile_func(&modules[0], 1, &agent).unwrap();
///
/// assert_eq!(outcome.func_name, "deposit");
/// assert!(outcome.verified);
/// ```
pub fn transpile_func(
    module: &Module,
    func_idx: usize,
    agent: &dyn CodeAgent,
) -> Result<FuncOutcome, String> {
    let func = match &module.items[func_idx] {
        Item::Func(f) => f.clone(),
        _ => return Err("transpile_func requires a function item".into()),
    };
    let spec = FuncSpec::new(module.attributes.clone(), func.clone());
    let target = tpt_telos_router::route(&module.attributes).target;

    let mut candidate = if func.elided || func.body.is_empty() {
        agent.generate(&spec)?
    } else {
        // Start from the developer's stated body; verify it before trusting it.
        Candidate {
            stmts: func.body.clone(),
        }
    };

    let mut iterations = Vec::new();

    for iter in 0..MAX_ITERS {
        let problem = problem_for(module, func_idx, &candidate.stmts)?;
        let result = verify(&problem);
        let passed = result.all_passed;

        iterations.push(LoopStep {
            iteration: iter,
            action: if iter == 0 {
                "generate".to_string()
            } else {
                "rewrite".to_string()
            },
            candidate: candidate.clone(),
            passed,
            counterexample: if passed {
                None
            } else {
                find_counterexample(&problem, &result)
            },
        });

        if passed {
            return Ok(FuncOutcome {
                func_name: func.name.clone(),
                target,
                agent: agent.name().to_string(),
                iterations,
                final_candidate: candidate,
                problem,
                result,
                verified: true,
            });
        }

        let ce = find_counterexample(&problem, &result).unwrap_or_default();
        let repaired = agent.rewrite(&spec, &candidate, &ce)?;
        if repaired.stmts == candidate.stmts {
            // Agent could not improve further; synthesize from the contract as a
            // correct-by-construction fallback so the loop terminates.
            let syn = static_agent::synthesize_from_ensures(&spec);
            let syn_problem = problem_for(module, func_idx, &syn.stmts)?;
            let syn_result = verify(&syn_problem);
            iterations.push(LoopStep {
                iteration: iter + 1,
                action: "synthesize-from-ensures".to_string(),
                candidate: syn.clone(),
                passed: syn_result.all_passed,
                counterexample: None,
            });
            return Ok(FuncOutcome {
                func_name: func.name.clone(),
                target,
                agent: agent.name().to_string(),
                iterations,
                final_candidate: syn,
                problem: syn_problem,
                result: syn_result.clone(),
                verified: syn_result.all_passed,
            });
        }
        candidate = repaired;
    }

    // Loop exhausted without a proof; return the last candidate for diagnostics.
    let problem = problem_for(module, func_idx, &candidate.stmts)?;
    let result = verify(&problem);
    Ok(FuncOutcome {
        func_name: func.name.clone(),
        target,
        agent: agent.name().to_string(),
        iterations,
        final_candidate: candidate,
        problem,
        result,
        verified: false,
    })
}

/// Convenience: transpile every function in a module and collect outcomes.
///
/// # Examples
///
/// ```
/// use tpt_telos_parser::parse;
/// use tpt_telos_agent::{transpile_module, StaticAgent};
///
/// let src = r#"
///     module Bank {
///         invariant Wallet { balance >= 0 }
///         func deposit(w: Wallet, amount: PositiveInt)
///             ensures w.balance == old(w.balance) + amount
///         ;
///     }
/// "#;
///
/// let modules = parse(src).unwrap();
/// let outcomes = transpile_module(&modules[0], &StaticAgent::new()).unwrap();
///
/// assert_eq!(outcomes.len(), 1);
/// assert!(outcomes[0].verified);
/// ```
pub fn transpile_module(
    module: &Module,
    agent: &dyn CodeAgent,
) -> Result<Vec<FuncOutcome>, String> {
    let mut out = Vec::new();
    for (idx, item) in module.items.iter().enumerate() {
        if let Item::Func(_) = item {
            out.push(transpile_func(module, idx, agent)?);
        }
    }
    Ok(out)
}

/// Convenience helper used by codegen: render a [`Candidate`] body back into
/// source text via the parser's pretty-printer.
///
/// # Examples
///
/// ```
/// use tpt_telos_parser::parse;
/// use tpt_telos_agent::{StaticAgent, transpile_module, render_candidate};
///
/// let src = r#"
///     module M {
///         func noop(w: Wallet, amount: PositiveInt)
///             ensures w.balance == old(w.balance) + amount
///         ;
///     }
/// "#;
///
/// let modules = parse(src).unwrap();
/// let outcomes = transpile_module(&modules[0], &StaticAgent::new()).unwrap();
/// let text = render_candidate(&outcomes[0].final_candidate);
/// assert!(text.contains("w.balance"));
/// ```
pub fn render_candidate(c: &Candidate) -> String {
    let mut s = String::new();
    for stmt in &c.stmts {
        s.push_str(&render_stmt(stmt));
        s.push('\n');
    }
    s
}

fn render_stmt(s: &Stmt) -> String {
    match s {
        Stmt::MutateState(assigns) => {
            let inner = assigns
                .iter()
                .map(render_assign)
                .collect::<Vec<_>>()
                .join("\n");
            format!("mutate state {{\n{}\n}}", indent(&inner))
        }
        Stmt::Assign(a) => render_assign(a),
        Stmt::Let(lb) => {
            let ty = lb
                .ty
                .as_ref()
                .map(|t| format!(": {}", render_type(t)))
                .unwrap_or_default();
            format!("let {}{} = {};", lb.name, ty, pretty(&lb.value))
        }
        Stmt::If(is) => {
            let mut out = format!("if {} {{\n", pretty(&is.condition));
            for s in &is.then_body {
                out.push_str(&indent(&render_stmt(s)));
                out.push('\n');
            }
            out.push('}');
            if let Some(else_body) = &is.else_body {
                out.push_str(" else {\n");
                for s in else_body {
                    out.push_str(&indent(&render_stmt(s)));
                    out.push('\n');
                }
                out.push('}');
            }
            out
        }
        Stmt::Match(ms) => {
            let mut out = format!("match {} {{\n", pretty(&ms.scrutinee));
            for arm in &ms.arms {
                out.push_str(&format!("    {} => {{\n", render_pattern(&arm.pattern)));
                for s in &arm.body {
                    out.push_str(&indent(&render_stmt(s)));
                    out.push('\n');
                }
                out.push_str("    }\n");
            }
            out.push('}');
            out
        }
        Stmt::Return(e) => match e {
            Some(expr) => format!("return {};", pretty(expr)),
            None => "return;".to_string(),
        },
    }
}

fn render_assign(a: &Assign) -> String {
    let op = match a.op {
        AssignOp::Set => "=",
        AssignOp::Add => "+=",
        AssignOp::Sub => "-=",
    };
    format!("{} {} {};", pretty(&a.target), op, pretty(&a.value))
}

fn pretty(e: &Expr) -> String {
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Var(v) => v.clone(),
        Expr::Field { base, field } => format!("{}.{}", base, field),
        Expr::Old(inner) => format!("old({})", pretty(inner)),
        Expr::Unary { op, expr } => match op {
            UnOp::Neg => format!("-{}", pretty(expr)),
        },
        Expr::Bin { op, lhs, rhs } => {
            let s = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Eq => "==",
                BinOp::Ne => "!=",
                BinOp::Lt => "<",
                BinOp::Le => "<=",
                BinOp::Gt => ">",
                BinOp::Ge => ">=",
                BinOp::And => "&&",
                BinOp::Or => "||",
            };
            format!("{} {} {}", pretty(lhs), s, pretty(rhs))
        }
        Expr::Call(c) => {
            let args: Vec<_> = c.args.iter().map(pretty).collect();
            format!("{}({})", c.func, args.join(", "))
        }
        Expr::MethodCall(m) => {
            let args: Vec<_> = m.args.iter().map(pretty).collect();
            format!("{}.{}({})", pretty(&m.receiver), m.method, args.join(", "))
        }
        Expr::Index(i) => format!("{}[{}]", pretty(&i.receiver), pretty(&i.index)),
        Expr::If(i) => format!(
            "if {} {{ {} }} else {{ {} }}",
            pretty(&i.condition),
            pretty(&i.then_expr),
            pretty(&i.else_expr)
        ),
        Expr::Match(m) => {
            let arms: Vec<_> = m
                .arms
                .iter()
                .map(|a| format!("... => {}", pretty(&a.expr)))
                .collect();
            format!("match {} {{ {} }}", pretty(&m.scrutinee), arms.join(", "))
        }
        Expr::Try(e) => format!("{}?", pretty(e)),
        Expr::Forall(f) => format!(
            "forall {}: {} {{ {} }}",
            f.var,
            f.var_ty.name(),
            pretty(&f.body)
        ),
        Expr::Aggregate(a) => {
            let args: Vec<_> = a.args.iter().map(pretty).collect();
            let op = match a.op {
                AggregateOp::Sum => "sum",
                AggregateOp::Min => "min",
                AggregateOp::Max => "max",
                AggregateOp::Count => "count",
            };
            format!("{}({})", op, args.join(", "))
        }
        Expr::Range { lo, hi } => format!("{}..{}", pretty(lo), pretty(hi)),
    }
}

fn render_type(t: &Type) -> String {
    match t {
        Type::Named(s) => match s.as_str() {
            "Float32" => "f32".to_string(),
            "Float64" => "f64".to_string(),
            "Int" => "i64".to_string(),
            "PositiveInt" => "i64".to_string(),
            other => other.to_string(),
        },
        Type::Generic(name, args) => {
            let args: Vec<_> = args.iter().map(render_type).collect();
            format!("{}<{}>", name, args.join(", "))
        }
        Type::Tuple(elems) => {
            let elems: Vec<_> = elems.iter().map(render_type).collect();
            format!("({})", elems.join(", "))
        }
        Type::Array(elem, len) => format!("[{}; {}]", render_type(elem), len),
        Type::Slice(elem) => format!("[{}]", render_type(elem)),
    }
}

fn render_pattern(p: &Pattern) -> String {
    match p {
        Pattern::Literal(n) => n.to_string(),
        Pattern::Var(v) => v.clone(),
        Pattern::Constructor(name, fields) => {
            if fields.is_empty() {
                name.clone()
            } else {
                let fields: Vec<_> = fields.iter().map(render_pattern).collect();
                format!("{}({})", name, fields.join(", "))
            }
        }
        Pattern::Wildcard => "_".to_string(),
    }
}

fn indent(s: &str) -> String {
    s.lines()
        .map(|line| {
            if line.is_empty() {
                line.to_string()
            } else {
                format!("    {}", line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
