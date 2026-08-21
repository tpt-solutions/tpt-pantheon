//! Context router for tpt-telos.
//!
//! Decides, from `@boundary(...)` architectural metadata, whether a module or
//! function should be compiled to the Rust backend (CPU-bound, zero-allocation,
//! cryptographic paths) or the Go backend (network I/O, high-concurrency,
//! distributed paths).
//!
//! In Phase 2 only the Rust backend is generated; the router nonetheless
//! records the intended target so the dual-backend work (Phase 3) can consume
//! it without further parsing changes.

use tpt_telos_parser::ast::{Arg, Attribute};

/// The compilation backend a module or function is routed to.
///
/// Determined by `@boundary(...)` architectural metadata.
/// Defaults to [`Target::Rust`] when no recognised flags are present.
///
/// # Examples
///
/// ```
/// use tpt_telos_router::{route, Target};
/// use tpt_telos_parser::ast::{Arg, Attribute};
///
/// let attr = Attribute {
///     name: "boundary".to_string(),
///     args: vec![Arg::Flag("network_io".to_string())],
/// };
/// let r = route(&[attr]);
/// assert_eq!(r.target, Target::Go);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Rust,
    Go,
    /// Python (with optional JAX/NumPy annotations) for ML/PINN workloads.
    /// Triggered by `@boundary(ml_training)`, `@boundary(python)`, or
    /// `@boundary(jax)`.
    Python,
}

impl Target {
    /// Returns the target as a lowercase string.
    ///
    /// # Examples
    ///
    /// ```
    /// use tpt_telos_router::Target;
    ///
    /// assert_eq!(Target::Rust.as_str(), "rust");
    /// assert_eq!(Target::Go.as_str(), "go");
    /// assert_eq!(Target::Python.as_str(), "python");
    /// ```
    pub fn as_str(&self) -> &'static str {
        match self {
            Target::Rust => "rust",
            Target::Go => "go",
            Target::Python => "python",
        }
    }
}

/// A routing warning emitted when architectural flags imply a potentially
/// unsafe target combination.
#[derive(Debug, Clone)]
pub struct RoutingDiagnostic {
    pub kind: DiagnosticKind,
    pub module: String,
    pub message: String,
}

/// The kind of routing conflict detected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// A `real_time` module was routed to Go, whose GC is non-deterministic.
    /// FADEC / PREEMPT_RT targets must stay on the Rust backend.
    RealTimeGoConflict,
    /// A `zero_allocation` module was routed to Go, which allocates via GC.
    ZeroAllocGoConflict,
    /// A `real_time` module was routed to Python (GC + interpreter).
    RealTimePythonConflict,
    /// A `zero_allocation` module was routed to Python (GC + interpreter).
    ZeroAllocPythonConflict,
    /// An `@state(...)` attribute contained an unrecognized value (possible typo).
    UnrecognizedStateValue,
    /// An `@boundary(...)` attribute contained an unrecognized flag (possible typo),
    /// so it was ignored and the module defaulted to Rust.
    UnrecognizedBoundaryFlag,
}

/// The storage class for a module's data structures, derived from
/// `@state(persistent)` or `@state(ephemeral)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StorageClass {
    /// Database-backed / serializable structs. `persistent` modules emit
    /// `#[derive(Serialize, Deserialize)]` (Rust) and JSON tags (Go).
    Persistent,
    /// Stack-only / transient structs (default). No serialization support.
    #[default]
    Ephemeral,
}

/// The routing decision for a module or function.
///
/// Contains the chosen [`Target`] backend and a human-readable `reason`
/// string for transparency in CLI output.
#[derive(Debug, Clone)]
pub struct Route {
    pub target: Target,
    /// The storage class derived from `@state(...)`.
    pub storage: StorageClass,
    /// Human-readable justification, surfaced by the CLI for transparency.
    pub reason: String,
}

impl Route {
    /// Returns `true` when the target is [`Target::Rust`].
    ///
    /// # Examples
    ///
    /// ```
    /// use tpt_telos_router::{route, Target};
    /// use tpt_telos_parser::ast::{Arg, Attribute};
    ///
    /// let cpu_attr = Attribute {
    ///     name: "boundary".to_string(),
    ///     args: vec![Arg::Flag("cpu_bound".to_string())],
    /// };
    /// let r = route(&[cpu_attr]);
    /// assert!(r.is_rust());
    /// assert_eq!(r.target, Target::Rust);
    /// ```
    pub fn is_rust(&self) -> bool {
        self.target == Target::Rust
    }
}

/// Boundary flags that imply a Rust target.
const RUST_FLAGS: &[&str] = &["cpu_bound", "zero_allocation", "crypto", "real_time"];

/// Boundary flags that imply a Go target.
const GO_FLAGS: &[&str] = &[
    "network_io",
    "high_concurrency",
    "distributed",
    "high_latency",
];

/// Boundary flags that imply a Python target (ML/JAX workloads).
const PYTHON_FLAGS: &[&str] = &["ml_training", "python", "jax"];

fn boundary_flags(attrs: &[Attribute]) -> Vec<String> {
    let mut flags = Vec::new();
    for attr in attrs {
        if attr.name != "boundary" {
            continue;
        }
        for arg in &attr.args {
            if let Arg::Flag(f) = arg {
                flags.push(f.clone());
            }
        }
    }
    flags
}

/// Parse `@state(persistent)` or `@state(ephemeral)` from module attributes.
/// Returns `(StorageClass, Option<unrecognized_value>)`. The second element is
/// `Some(value)` when a `@state(...)` attribute was present but the value was
/// not a recognized keyword (possible typo), so callers can emit a diagnostic.
fn parse_storage_class(attrs: &[Attribute]) -> (StorageClass, Option<String>) {
    for attr in attrs {
        if attr.name != "state" {
            continue;
        }
        for arg in &attr.args {
            if let Arg::Flag(f) = arg {
                match f.as_str() {
                    "persistent" => return (StorageClass::Persistent, None),
                    "ephemeral" => return (StorageClass::Ephemeral, None),
                    other => return (StorageClass::default(), Some(other.to_string())),
                }
            }
            if let Arg::Kv(k, tpt_telos_parser::ast::Literal::Ident(v)) = arg {
                if k == "class" {
                    match v.as_str() {
                        "persistent" => return (StorageClass::Persistent, None),
                        "ephemeral" => return (StorageClass::Ephemeral, None),
                        other => return (StorageClass::default(), Some(other.to_string())),
                    }
                }
            }
        }
    }
    (StorageClass::default(), None)
}

/// Route a module from its `@boundary` attributes and emit diagnostics for
/// potentially unsafe target combinations.
///
/// Diagnostics are non-fatal warnings; the caller decides whether to surface
/// them as errors (e.g. with `--strict-rt`).
///
/// # Examples
///
/// ```
/// use tpt_telos_router::{route_checked, Target, DiagnosticKind};
/// use tpt_telos_parser::ast::{Arg, Attribute};
///
/// let attr = Attribute {
///     name: "boundary".to_string(),
///     args: vec![
///         Arg::Flag("real_time".to_string()),
///         Arg::Flag("network_io".to_string()),
///     ],
/// };
/// let (r, diags) = route_checked(&[attr], "ControlLoop");
/// assert_eq!(r.target, Target::Go);
/// assert!(diags.iter().any(|d| d.kind == DiagnosticKind::RealTimeGoConflict));
/// ```
pub fn route_checked(attrs: &[Attribute], module_name: &str) -> (Route, Vec<RoutingDiagnostic>) {
    let flags = boundary_flags(attrs);

    // Parse @state(...) for storage class.
    let (storage, unrecognized_state) = parse_storage_class(attrs);

    let mut go_hits = Vec::new();
    let mut rust_hits = Vec::new();
    let mut python_hits = Vec::new();
    let mut unrecognized_flags = Vec::new();
    for f in &flags {
        if PYTHON_FLAGS.contains(&f.as_str()) {
            python_hits.push(f.clone());
        } else if GO_FLAGS.contains(&f.as_str()) {
            go_hits.push(f.clone());
        } else if RUST_FLAGS.contains(&f.as_str()) {
            rust_hits.push(f.clone());
        } else {
            unrecognized_flags.push(f.clone());
        }
    }

    // Python beats everything else (ML workloads are explicit opt-in).
    let route = if !python_hits.is_empty() {
        Route {
            target: Target::Python,
            storage,
            reason: format!("boundary flags {:?} routed to Python backend", python_hits),
        }
    } else if !go_hits.is_empty() {
        Route {
            target: Target::Go,
            storage,
            reason: format!("boundary flags {:?} routed to Go backend", go_hits),
        }
    } else if !rust_hits.is_empty() {
        Route {
            target: Target::Rust,
            storage,
            reason: format!("boundary flags {:?} routed to Rust backend", rust_hits),
        }
    } else {
        Route {
            target: Target::Rust,
            storage,
            reason: "no explicit boundary flags; defaulting to Rust compute backend".to_string(),
        }
    };

    let mut diagnostics = Vec::new();

    if let Some(ref bad_val) = unrecognized_state {
        diagnostics.push(RoutingDiagnostic {
            kind: DiagnosticKind::UnrecognizedStateValue,
            module: module_name.to_string(),
            message: format!(
                "module `{}` has unrecognized `@state({})` value; \
                 valid values are `persistent` and `ephemeral` (defaulting to ephemeral)",
                module_name, bad_val
            ),
        });
    }

    for bad_flag in &unrecognized_flags {
        diagnostics.push(RoutingDiagnostic {
            kind: DiagnosticKind::UnrecognizedBoundaryFlag,
            module: module_name.to_string(),
            message: format!(
                "module `{}` has unrecognized `@boundary({})` flag; \
                 valid flags are cpu_bound, zero_allocation, crypto, real_time, \
                 network_io, high_concurrency, distributed, high_latency, \
                 ml_training, python, jax (ignored; defaulting to Rust backend)",
                module_name, bad_flag
            ),
        });
    }

    if route.target == Target::Go {
        if rust_hits.iter().any(|f| f == "real_time") {
            diagnostics.push(RoutingDiagnostic {
                kind: DiagnosticKind::RealTimeGoConflict,
                module: module_name.to_string(),
                message: format!(
                    "module `{}` has `real_time` flag but is routed to Go (GC is \
                     non-deterministic; FADEC/PREEMPT_RT targets must use Rust; \
                     see ARCHITECTURE.md § Go GC Determinism)",
                    module_name
                ),
            });
        }
        if rust_hits.iter().any(|f| f == "zero_allocation") {
            diagnostics.push(RoutingDiagnostic {
                kind: DiagnosticKind::ZeroAllocGoConflict,
                module: module_name.to_string(),
                message: format!(
                    "module `{}` has `zero_allocation` flag but is routed to Go \
                     (Go allocates via GC; zero-allocation guarantees cannot be upheld; \
                     see ARCHITECTURE.md § Go GC Determinism)",
                    module_name
                ),
            });
        }
    }

    if route.target == Target::Python {
        if rust_hits.iter().any(|f| f == "real_time") {
            diagnostics.push(RoutingDiagnostic {
                kind: DiagnosticKind::RealTimePythonConflict,
                module: module_name.to_string(),
                message: format!(
                    "module `{}` has `real_time` flag but is routed to Python \
                     (Python is GC-based with non-deterministic latency; \
                     hard real-time targets must use Rust)",
                    module_name
                ),
            });
        }
        if rust_hits.iter().any(|f| f == "zero_allocation") {
            diagnostics.push(RoutingDiagnostic {
                kind: DiagnosticKind::ZeroAllocPythonConflict,
                module: module_name.to_string(),
                message: format!(
                    "module `{}` has `zero_allocation` flag but is routed to Python \
                     (Python allocates via GC; zero-allocation guarantees cannot be upheld)",
                    module_name
                ),
            });
        }
    }

    (route, diagnostics)
}

/// Route a module from its own `@boundary` attributes (and any inherited
/// function-level attributes if supplied). Thin wrapper around
/// [`route_checked`] that discards diagnostics for callers that do not need
/// them.
///
/// # Examples
///
/// ```
/// use tpt_telos_router::{route, Target};
/// use tpt_telos_parser::ast::{Arg, Attribute};
///
/// // No boundary flags → Rust (default).
/// assert_eq!(route(&[]).target, Target::Rust);
///
/// // cpu_bound → Rust.
/// let rust_attr = Attribute {
///     name: "boundary".to_string(),
///     args: vec![Arg::Flag("cpu_bound".to_string())],
/// };
/// assert_eq!(route(&[rust_attr]).target, Target::Rust);
///
/// // network_io → Go.
/// let go_attr = Attribute {
///     name: "boundary".to_string(),
///     args: vec![Arg::Flag("network_io".to_string())],
/// };
/// let r = route(&[go_attr]);
/// assert_eq!(r.target, Target::Go);
/// assert!(!r.is_rust());
/// assert!(r.reason.contains("network_io"));
///
/// // ml_training → Python.
/// let py_attr = Attribute {
///     name: "boundary".to_string(),
///     args: vec![Arg::Flag("ml_training".to_string())],
/// };
/// assert_eq!(route(&[py_attr]).target, Target::Python);
/// ```
pub fn route(attrs: &[Attribute]) -> Route {
    route_checked(attrs, "").0
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_telos_parser::ast::{Arg, Attribute};

    fn attr(flags: &[&str]) -> Attribute {
        Attribute {
            name: "boundary".to_string(),
            args: flags.iter().map(|f| Arg::Flag(f.to_string())).collect(),
        }
    }

    #[test]
    fn network_io_routes_to_go() {
        let r = route(&[attr(&["network_io"])]);
        assert_eq!(r.target, Target::Go);
    }

    #[test]
    fn cpu_bound_routes_to_rust() {
        let r = route(&[attr(&["cpu_bound"])]);
        assert_eq!(r.target, Target::Rust);
    }

    #[test]
    fn no_flags_default_rust() {
        let r = route(&[]);
        assert_eq!(r.target, Target::Rust);
    }

    // ---- every Rust-bound boundary flag ----

    #[test]
    fn zero_allocation_routes_to_rust() {
        let r = route(&[attr(&["zero_allocation"])]);
        assert_eq!(r.target, Target::Rust);
    }

    #[test]
    fn crypto_routes_to_rust() {
        let r = route(&[attr(&["crypto"])]);
        assert_eq!(r.target, Target::Rust);
    }

    #[test]
    fn real_time_routes_to_rust() {
        let r = route(&[attr(&["real_time"])]);
        assert_eq!(r.target, Target::Rust);
    }

    // ---- every Go-bound boundary flag ----

    #[test]
    fn high_concurrency_routes_to_go() {
        let r = route(&[attr(&["high_concurrency"])]);
        assert_eq!(r.target, Target::Go);
    }

    #[test]
    fn distributed_routes_to_go() {
        let r = route(&[attr(&["distributed"])]);
        assert_eq!(r.target, Target::Go);
    }

    #[test]
    fn high_latency_routes_to_go() {
        let r = route(&[attr(&["high_latency"])]);
        assert_eq!(r.target, Target::Go);
    }

    // ---- combinations ----

    #[test]
    fn multiple_rust_flags_still_rust() {
        let r = route(&[attr(&["cpu_bound", "zero_allocation", "crypto"])]);
        assert_eq!(r.target, Target::Rust);
    }

    #[test]
    fn multiple_go_flags_still_go() {
        let r = route(&[attr(&["network_io", "high_concurrency"])]);
        assert_eq!(r.target, Target::Go);
    }

    #[test]
    fn go_flag_wins_over_rust_flag() {
        // A mixed annotation is treated as a Go (distributed/network) target.
        let r = route(&[attr(&["cpu_bound", "network_io"])]);
        assert_eq!(r.target, Target::Go);
    }

    #[test]
    fn unrecognised_flag_defaults_to_rust() {
        // Unknown flags contribute nothing; the default compute backend applies.
        let r = route(&[attr(&["some_future_flag"])]);
        assert_eq!(r.target, Target::Rust);
    }

    #[test]
    fn unrecognised_boundary_flag_emits_diagnostic() {
        let (r, diags) = route_checked(&[attr(&["some_future_flag"])], "MyModule");
        assert_eq!(r.target, Target::Rust);
        assert!(diags
            .iter()
            .any(|d| d.kind == DiagnosticKind::UnrecognizedBoundaryFlag));
    }

    #[test]
    fn route_records_justification() {
        let r = route(&[attr(&["network_io"])]);
        assert!(r.reason.contains("network_io"));
        assert!(r.is_rust() == (r.target == Target::Rust));
    }

    // ---- Python target ----

    #[test]
    fn ml_training_routes_to_python() {
        let r = route(&[attr(&["ml_training"])]);
        assert_eq!(r.target, Target::Python);
    }

    #[test]
    fn python_flag_routes_to_python() {
        let r = route(&[attr(&["python"])]);
        assert_eq!(r.target, Target::Python);
    }

    #[test]
    fn jax_flag_routes_to_python() {
        let r = route(&[attr(&["jax"])]);
        assert_eq!(r.target, Target::Python);
    }

    #[test]
    fn python_beats_go_flag() {
        // ml_training takes priority over network_io.
        let r = route(&[attr(&["ml_training", "network_io"])]);
        assert_eq!(r.target, Target::Python);
    }

    // ---- Real-time conflict diagnostics ----

    #[test]
    fn real_time_go_conflict_emits_diagnostic() {
        let (r, diags) = route_checked(&[attr(&["real_time", "network_io"])], "ControlLoop");
        assert_eq!(r.target, Target::Go);
        assert!(diags
            .iter()
            .any(|d| d.kind == DiagnosticKind::RealTimeGoConflict));
    }

    #[test]
    fn zero_alloc_go_conflict_emits_diagnostic() {
        let (r, diags) = route_checked(&[attr(&["zero_allocation", "high_concurrency"])], "Buffer");
        assert_eq!(r.target, Target::Go);
        assert!(diags
            .iter()
            .any(|d| d.kind == DiagnosticKind::ZeroAllocGoConflict));
    }

    #[test]
    fn real_time_alone_has_no_diagnostics() {
        let (r, diags) = route_checked(&[attr(&["real_time"])], "FADEC");
        assert_eq!(r.target, Target::Rust);
        assert!(diags.is_empty());
    }

    #[test]
    fn python_target_has_no_diagnostics() {
        let (r, diags) = route_checked(&[attr(&["ml_training"])], "TrainLayer");
        assert_eq!(r.target, Target::Python);
        assert!(diags.is_empty());
    }

    // ---- Storage class ----

    fn state_attr(class: &str) -> Attribute {
        Attribute {
            name: "state".to_string(),
            args: vec![Arg::Flag(class.to_string())],
        }
    }

    #[test]
    fn state_persistent_sets_storage_class() {
        let r = route(&[state_attr("persistent")]);
        assert_eq!(r.storage, StorageClass::Persistent);
    }

    #[test]
    fn state_ephemeral_sets_storage_class() {
        let r = route(&[state_attr("ephemeral")]);
        assert_eq!(r.storage, StorageClass::Ephemeral);
    }

    #[test]
    fn no_state_defaults_to_ephemeral() {
        let r = route(&[]);
        assert_eq!(r.storage, StorageClass::Ephemeral);
    }

    #[test]
    fn state_with_boundary_combines() {
        let boundary = attr(&["network_io"]);
        let state = state_attr("persistent");
        let r = route(&[boundary, state]);
        assert_eq!(r.target, Target::Go);
        assert_eq!(r.storage, StorageClass::Persistent);
    }
}
