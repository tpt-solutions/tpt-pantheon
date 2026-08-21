//! tpt-telos formal verifier (QF_LRA).
//!
//! Provides a self-contained SMT-style engine that decides whether a set of
//! linear arithmetic constraints is unsatisfiable, and uses it to prove that
//! each `ensures` clause and invariant of a function follows from its
//! `requires` and `mutate state` assignments.
//!
//! The default solver is an internal Fourier-Motzkin variable elimination
//! engine. When the `z3` feature is enabled, an alternative Z3-backed solver
//! is available via [`SolverBackend::Z3`].

pub mod cluster;
pub mod solver;
pub mod verify;

#[cfg(feature = "z3")]
pub mod z3_solver;

pub use solver::{
    counterexample, entails, equality_substitute, model, negate, satisfies_model, unsat,
    unsat_checked, Model,
};
pub use verify::{is_unsat, verify, CheckResult, VerificationResult};

/// The solver backend to use for verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SolverBackend {
    /// Built-in Fourier-Motzkin variable elimination (default, no external
    /// dependencies).
    #[default]
    FourierMotzkin,
    /// Z3 SMT solver (requires the `z3` feature and the Z3 shared library
    /// to be installed).
    #[cfg(feature = "z3")]
    Z3,
}

use std::sync::OnceLock;

static SOLVER_BACKEND: OnceLock<SolverBackend> = OnceLock::new();

/// Set the global solver backend. Has no effect if already set.
pub fn set_solver_backend(backend: SolverBackend) {
    let _ = SOLVER_BACKEND.set(backend);
}

/// Get the current solver backend.
pub fn solver_backend() -> SolverBackend {
    SOLVER_BACKEND.get().copied().unwrap_or_default()
}
