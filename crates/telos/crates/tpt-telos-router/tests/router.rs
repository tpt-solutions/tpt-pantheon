use tpt_telos_parser::ast::{Arg, Attribute};
use tpt_telos_router::{route, route_checked, DiagnosticKind, StorageClass, Target};

fn flag_attr(flag: &str) -> Attribute {
    Attribute {
        name: "boundary".to_string(),
        args: vec![Arg::Flag(flag.to_string())],
    }
}

fn flags_attr(flags: &[&str]) -> Attribute {
    Attribute {
        name: "boundary".to_string(),
        args: flags
            .iter()
            .map(|f| Arg::Flag(f.to_string()))
            .collect(),
    }
}

fn state_attr(value: &str) -> Attribute {
    Attribute {
        name: "state".to_string(),
        args: vec![Arg::Flag(value.to_string())],
    }
}

// ---------------------------------------------------------------------------
// Default routing
// ---------------------------------------------------------------------------

#[test]
fn no_attrs_defaults_to_rust() {
    let r = route(&[]);
    assert_eq!(r.target, Target::Rust);
    assert_eq!(r.storage, StorageClass::Ephemeral);
}

#[test]
fn unrecognized_boundary_flag_defaults_to_rust() {
    let r = route(&[flag_attr("unknown_flag")]);
    assert_eq!(r.target, Target::Rust);
}

// ---------------------------------------------------------------------------
// Rust flags
// ---------------------------------------------------------------------------

#[test]
fn cpu_bound_routes_rust() {
    assert_eq!(route(&[flag_attr("cpu_bound")]).target, Target::Rust);
}

#[test]
fn zero_allocation_routes_rust() {
    assert_eq!(route(&[flag_attr("zero_allocation")]).target, Target::Rust);
}

#[test]
fn crypto_routes_rust() {
    assert_eq!(route(&[flag_attr("crypto")]).target, Target::Rust);
}

#[test]
fn real_time_routes_rust() {
    assert_eq!(route(&[flag_attr("real_time")]).target, Target::Rust);
}

// ---------------------------------------------------------------------------
// Go flags
// ---------------------------------------------------------------------------

#[test]
fn network_io_routes_go() {
    assert_eq!(route(&[flag_attr("network_io")]).target, Target::Go);
}

#[test]
fn high_concurrency_routes_go() {
    assert_eq!(route(&[flag_attr("high_concurrency")]).target, Target::Go);
}

#[test]
fn distributed_routes_go() {
    assert_eq!(route(&[flag_attr("distributed")]).target, Target::Go);
}

#[test]
fn high_latency_routes_go() {
    assert_eq!(route(&[flag_attr("high_latency")]).target, Target::Go);
}

// ---------------------------------------------------------------------------
// Python flags
// ---------------------------------------------------------------------------

#[test]
fn ml_training_routes_python() {
    assert_eq!(route(&[flag_attr("ml_training")]).target, Target::Python);
}

#[test]
fn python_flag_routes_python() {
    assert_eq!(route(&[flag_attr("python")]).target, Target::Python);
}

#[test]
fn jax_routes_python() {
    assert_eq!(route(&[flag_attr("jax")]).target, Target::Python);
}

// ---------------------------------------------------------------------------
// Priority: Python > Go > Rust
// ---------------------------------------------------------------------------

#[test]
fn python_beats_go() {
    let attr = flags_attr(&["network_io", "ml_training"]);
    assert_eq!(route(&[attr]).target, Target::Python);
}

#[test]
fn python_beats_rust() {
    let attr = flags_attr(&["cpu_bound", "python"]);
    assert_eq!(route(&[attr]).target, Target::Python);
}

#[test]
fn go_beats_rust() {
    let attr = flags_attr(&["cpu_bound", "network_io"]);
    assert_eq!(route(&[attr]).target, Target::Go);
}

#[test]
fn python_beats_go_and_rust() {
    let attr = flags_attr(&["cpu_bound", "distributed", "jax"]);
    assert_eq!(route(&[attr]).target, Target::Python);
}

// ---------------------------------------------------------------------------
// Conflict diagnostics via route_checked
// ---------------------------------------------------------------------------

#[test]
fn real_time_plus_go_emits_conflict() {
    let attr = flags_attr(&["real_time", "network_io"]);
    let (r, diags) = route_checked(&[attr], "ControlLoop");
    assert_eq!(r.target, Target::Go);
    assert!(
        diags.iter().any(|d| d.kind == DiagnosticKind::RealTimeGoConflict),
        "expected RealTimeGoConflict diagnostic"
    );
}

#[test]
fn zero_alloc_plus_go_emits_conflict() {
    let attr = flags_attr(&["zero_allocation", "network_io"]);
    let (r, diags) = route_checked(&[attr], "ZeroAllocNet");
    assert_eq!(r.target, Target::Go);
    assert!(
        diags.iter().any(|d| d.kind == DiagnosticKind::ZeroAllocGoConflict),
        "expected ZeroAllocGoConflict diagnostic"
    );
}

#[test]
fn real_time_plus_python_emits_conflict() {
    let attr = flags_attr(&["real_time", "ml_training"]);
    let (r, diags) = route_checked(&[attr], "RtML");
    assert_eq!(r.target, Target::Python);
    assert!(
        diags.iter().any(|d| d.kind == DiagnosticKind::RealTimePythonConflict),
        "expected RealTimePythonConflict diagnostic"
    );
}

#[test]
fn zero_alloc_plus_python_emits_conflict() {
    let attr = flags_attr(&["zero_allocation", "python"]);
    let (r, diags) = route_checked(&[attr], "ZeroAllocPy");
    assert_eq!(r.target, Target::Python);
    assert!(
        diags.iter().any(|d| d.kind == DiagnosticKind::ZeroAllocPythonConflict),
        "expected ZeroAllocPythonConflict diagnostic"
    );
}

#[test]
fn unrecognized_flag_emits_diagnostic() {
    let attr = flag_attr("turbo_mode");
    let (r, diags) = route_checked(&[attr], "MyModule");
    assert_eq!(r.target, Target::Rust);
    assert!(
        diags.iter().any(|d| d.kind == DiagnosticKind::UnrecognizedBoundaryFlag),
        "expected UnrecognizedBoundaryFlag diagnostic"
    );
}

#[test]
fn clean_rust_module_has_no_diagnostics() {
    let attr = flag_attr("cpu_bound");
    let (r, diags) = route_checked(&[attr], "CpuModule");
    assert_eq!(r.target, Target::Rust);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

#[test]
fn clean_go_module_has_no_diagnostics() {
    let attr = flag_attr("distributed");
    let (r, diags) = route_checked(&[attr], "DistModule");
    assert_eq!(r.target, Target::Go);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

// ---------------------------------------------------------------------------
// Storage class from @state(...)
// ---------------------------------------------------------------------------

#[test]
fn no_state_attr_defaults_ephemeral() {
    let r = route(&[flag_attr("cpu_bound")]);
    assert_eq!(r.storage, StorageClass::Ephemeral);
}

#[test]
fn persistent_state_attr() {
    let attrs = [flag_attr("network_io"), state_attr("persistent")];
    let r = route(&attrs);
    assert_eq!(r.storage, StorageClass::Persistent);
    assert_eq!(r.target, Target::Go);
}

#[test]
fn ephemeral_state_attr_explicit() {
    let attrs = [flag_attr("cpu_bound"), state_attr("ephemeral")];
    let r = route(&attrs);
    assert_eq!(r.storage, StorageClass::Ephemeral);
}

#[test]
fn unrecognized_state_value_emits_diagnostic() {
    let attrs = [flag_attr("cpu_bound"), state_attr("volatile")];
    let (r, diags) = route_checked(&attrs, "MyModule");
    assert_eq!(r.storage, StorageClass::Ephemeral);
    assert!(
        diags.iter().any(|d| d.kind == DiagnosticKind::UnrecognizedStateValue),
        "expected UnrecognizedStateValue diagnostic"
    );
}

// ---------------------------------------------------------------------------
// route() vs route_checked() consistency
// ---------------------------------------------------------------------------

#[test]
fn route_and_route_checked_agree_on_target() {
    let flags = [
        "cpu_bound",
        "network_io",
        "ml_training",
        "real_time",
        "distributed",
        "jax",
    ];
    for f in flags {
        let attr = flag_attr(f);
        let simple = route(&[attr.clone()]);
        let (checked, _) = route_checked(&[attr], "M");
        assert_eq!(
            simple.target, checked.target,
            "route() and route_checked() disagree for flag `{f}`"
        );
    }
}

// ---------------------------------------------------------------------------
// is_rust() helper
// ---------------------------------------------------------------------------

#[test]
fn is_rust_true_for_rust_target() {
    assert!(route(&[flag_attr("cpu_bound")]).is_rust());
}

#[test]
fn is_rust_false_for_go_target() {
    assert!(!route(&[flag_attr("network_io")]).is_rust());
}

#[test]
fn is_rust_false_for_python_target() {
    assert!(!route(&[flag_attr("ml_training")]).is_rust());
}

// ---------------------------------------------------------------------------
// Diagnostic fields
// ---------------------------------------------------------------------------

#[test]
fn conflict_diagnostic_carries_module_name() {
    let attr = flags_attr(&["real_time", "distributed"]);
    let (_, diags) = route_checked(&[attr], "FlightControl");
    let conflict = diags
        .iter()
        .find(|d| d.kind == DiagnosticKind::RealTimeGoConflict)
        .expect("expected RealTimeGoConflict");
    assert_eq!(conflict.module, "FlightControl");
    assert!(!conflict.message.is_empty());
}

// ---------------------------------------------------------------------------
// Multiple flags of the same kind
// ---------------------------------------------------------------------------

#[test]
fn multiple_rust_flags_still_rust() {
    let attr = flags_attr(&["cpu_bound", "crypto", "real_time"]);
    assert_eq!(route(&[attr]).target, Target::Rust);
}

#[test]
fn multiple_go_flags_still_go() {
    let attr = flags_attr(&["network_io", "high_concurrency", "distributed"]);
    assert_eq!(route(&[attr]).target, Target::Go);
}

// ---------------------------------------------------------------------------
// Non-boundary attributes are ignored
// ---------------------------------------------------------------------------

#[test]
fn non_boundary_attrs_ignored() {
    let attr = Attribute {
        name: "eject".to_string(),
        args: vec![],
    };
    let r = route(&[attr]);
    assert_eq!(r.target, Target::Rust);
}
