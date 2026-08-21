//! Programmatic orchestration API for tpt-telos.
//!
//! `tpt-nexus` (and other integration harnesses) need a single library entry
//! point over the parse -> agentic-transpile -> codegen -> attest pipeline
//! instead of depending on six crates directly and hand-rolling their own
//! orchestrator. This crate closes exactly those gaps:
//!
//! * [`compile`] / [`compile_static`] — one call from `.telos` source (or bytes)
//!   to a fully-orchestrated [`VerifiedArtifact`].
//! * [`format_hint`] / [`format_outcome_hints`] — human/LLM-readable
//!   counterexample hints (the "agent_hint" concept `tpt-nexus` flagged as
//!   missing from tpt-telos).
//! * [`compile_project`] / [`compile_project_tempdir`] — write a [`Project`] to
//!   disk and shell `cargo build` / `go build`, reading back the compiled Rust
//!   artifact bytes.
//! * [`check_contradictions`] — a `.telos`-source-independent entry point over
//!   just the solver core: find contradictions among named groups of
//!   [`Constraint`]s, for callers (e.g. an alerting-rule engine) that have
//!   their own domain rules but no `.telos` source to verify.
//!
//! Pipeline errors are surfaced as [`SdkError`]; verification *failure* is **not**
//! an error — it is reported via [`VerifiedArtifact::all_verified`].

pub mod build;
pub mod contradiction;
pub mod error;
pub mod hint;
pub mod json;
pub mod prove;

/// The fully-orchestrated result of compiling a `.telos` source: parsed
/// modules, per-function transpilation outcomes, the assembled dual-backend
/// [`Project`], the attestation [`ProofManifest`], and an overall
/// `all_verified` flag.
///
/// `Clone`-only: [`tpt_telos_agent::FuncOutcome`] does not implement `Debug`.
#[derive(Clone)]
pub struct VerifiedArtifact {
    /// The original source bytes (kept so a manifest can be re-derived/checked).
    pub source: Vec<u8>,
    /// Parsed modules.
    pub modules: Vec<tpt_telos_parser::ast::Module>,
    /// Per-function transpilation outcomes (one per `FuncOutcome`).
    pub outcomes: Vec<tpt_telos_agent::FuncOutcome>,
    /// The assembled dual-backend project (Rust/Go/FFI sources).
    pub project: tpt_telos_codegen::project::Project,
    /// The attestation manifest (source hash + per-function verification record).
    pub manifest: tpt_telos_codegen::proof::ProofManifest,
    /// True only when *every* function verified against its contracts.
    pub all_verified: bool,
}

/// Run the full pipeline over `.telos` source text.
///
/// `Err` is returned only when the pipeline itself could not run (parse,
/// agent, or codegen failure). A program whose contracts are not satisfied
/// still returns `Ok(...)` with `all_verified == false`.
///
/// # Examples
///
/// ```
/// use tpt_telos_sdk::{compile, StaticAgent};
///
/// let src = r#"
///     module Bank {
///         invariant Wallet { balance >= 0 }
///         func deposit(w: Wallet, amount: PositiveInt)
///             ensures w.balance == old(w.balance) + amount
///         ;
///     }
/// "#;
/// let artifact = compile(src, &StaticAgent::new()).unwrap();
/// assert!(artifact.all_verified);
/// ```
pub fn compile(
    source: &str,
    agent: &dyn tpt_telos_agent::CodeAgent,
) -> Result<VerifiedArtifact, SdkError> {
    let modules = tpt_telos_parser::parse(source).map_err(SdkError::Parse)?;
    let mut outcomes: Vec<tpt_telos_agent::FuncOutcome> = Vec::new();
    for m in &modules {
        let funcs = tpt_telos_agent::transpile_module(m, agent).map_err(SdkError::Transpile)?;
        outcomes.extend(funcs);
    }
    let project =
        tpt_telos_codegen::generate_project(&modules, &outcomes).map_err(SdkError::Codegen)?;
    let manifest = tpt_telos_codegen::proof::generate_manifest(source.as_bytes(), &outcomes);
    let all_verified = outcomes.iter().all(|o| o.verified);
    Ok(VerifiedArtifact {
        source: source.as_bytes().to_vec(),
        modules,
        outcomes,
        project,
        manifest,
        all_verified,
    })
}

/// Like [`compile`] but accepts raw source bytes (e.g. read from disk).
///
/// # Examples
///
/// ```
/// use tpt_telos_sdk::{compile_static, StaticAgent};
///
/// let src = b"module Bank { func noop() ; }";
/// let artifact = compile_static(src, &StaticAgent::new()).unwrap();
/// assert!(artifact.all_verified);
/// ```
pub fn compile_static(
    source: &[u8],
    agent: &dyn tpt_telos_agent::CodeAgent,
) -> Result<VerifiedArtifact, SdkError> {
    let text = std::str::from_utf8(source)
        .map_err(|e| SdkError::Parse(format!("source is not valid UTF-8: {e}")))?;
    compile(text, agent)
}

// ---------------------------------------------------------------------------
// Flat re-exports: consumers need these type names without depending on all six
// wrapped crates directly.
// ---------------------------------------------------------------------------

pub use error::SdkError;

#[cfg(feature = "llm")]
pub use tpt_telos_agent::llm_agent::LlmAgent;
pub use tpt_telos_agent::{CodeAgent, FuncOutcome, StaticAgent};
pub use tpt_telos_codegen::project::Project;
pub use tpt_telos_codegen::proof::ProofManifest;
pub use tpt_telos_ir::{Constraint, Linear, Relation};
pub use tpt_telos_parser::ast::Module;
pub use tpt_telos_router::Target;
pub use tpt_telos_verifier::{
    entails, is_unsat, model, unsat_checked, CheckResult, Model, VerificationResult,
};

pub use crate::build::{compile_project, compile_project_tempdir, BuildOutput};
pub use crate::contradiction::{
    check_contradictions, Contradiction, ContradictionReport, NamedConstraints,
};
pub use crate::hint::{format_hint, format_outcome_hints};
pub use crate::json::{parse_json, JsonError, JsonValue};
pub use crate::prove::{build_report, parse_groups, ProveReport};
