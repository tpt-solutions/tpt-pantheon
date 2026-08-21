//! Integration tests for the `telos` CLI binary.
//!
//! Drives the compiled binary (`CARGO_BIN_EXE_telos`) for `verify`, `transpile`,
//! and `build`, covering success, verification-failure, and malformed-file exit
//! codes and output.

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_telos");
const WALLET: &str = "../../examples/wallet.telos";
const BROKEN: &str = "../../examples/broken.telos";

fn run(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(BIN)
        .args(args)
        .output()
        .expect("failed to run telos");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    (out.status.success(), stdout, stderr)
}

fn write_temp(name: &str, contents: &str) -> PathBuf {
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn verify_succeeds_on_valid_module() {
    let (ok, stdout, _) = run(&["verify", WALLET]);
    assert!(ok, "verify should exit 0 on a valid module");
    assert!(
        stdout.contains("PASS"),
        "expected PASS in output:\n{stdout}"
    );
    assert!(
        stdout.contains("all constraints satisfied"),
        "expected success summary:\n{stdout}"
    );
}

#[test]
fn verify_fails_on_broken_module() {
    let (ok, stdout, _) = run(&["verify", BROKEN]);
    assert!(!ok, "verify should exit non-zero on a failing module");
    assert!(
        stdout.contains("FAIL"),
        "expected FAIL in output:\n{stdout}"
    );
    assert!(
        stdout.contains("verification failed"),
        "expected failure summary:\n{stdout}"
    );
}

#[test]
fn verify_reports_error_on_malformed_file() {
    let path = write_temp(
        "telos_cli_malformed.telos",
        "module M { func f(x: T) requires @ }",
    );
    let (ok, _, stderr) = run(&["verify", path.to_str().unwrap()]);
    assert!(!ok, "verify should exit non-zero on a malformed file");
    assert!(
        stderr.contains("error"),
        "expected an error on stderr:\n{stderr}"
    );
}

#[test]
fn transpile_emits_rust_to_stdout() {
    let (ok, stdout, stderr) = run(&["transpile", WALLET]);
    assert!(ok, "transpile should exit 0: {stderr}");
    assert!(
        stdout.contains("pub fn transfer"),
        "expected generated Rust:\n{stdout}"
    );
}

#[test]
fn transpile_writes_to_output_file() {
    let path = std::env::temp_dir().join("telos_cli_transpile_out.rs");
    let (ok, _, stderr) = run(&["transpile", WALLET, "--out", path.to_str().unwrap()]);
    assert!(ok, "transpile --out should exit 0: {stderr}");
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("pub fn transfer"),
        "output file missing generated Rust:\n{written}"
    );
}

#[test]
fn build_compiles_generated_rust() {
    // Use a unique temp dir so parallel test runs don't collide.
    let dir = std::env::temp_dir().join(format!("telos_cli_build_{}", std::process::id()));
    let (ok, stdout, stderr) = run(&["build", WALLET, "--out-dir", dir.to_str().unwrap()]);
    assert!(ok, "build should exit 0:\n{stderr}");
    assert!(
        stdout.contains("compiles successfully"),
        "expected successful compile:\n{stdout}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn parse_reports_error_on_malformed_file() {
    let path = write_temp("telos_cli_malformed2.telos", "module M { func f(@) }");
    let (ok, _, stderr) = run(&["parse", path.to_str().unwrap()]);
    assert!(!ok, "parse should exit non-zero on a malformed file");
    assert!(stderr.contains("error"), "expected an error: {stderr}");
}

#[test]
fn project_generates_dual_backend_tree() {
    let dir = std::env::temp_dir().join(format!("telos_cli_project_{}", std::process::id()));
    let (ok, stdout, stderr) = run(&[
        "project",
        "../../examples/microservice.telos",
        "--out-dir",
        dir.to_str().unwrap(),
    ]);
    assert!(ok, "project should exit 0: {stderr}");
    assert!(
        stdout.contains("Backends: rust=true go=true"),
        "expected dual-backend summary:\n{stdout}"
    );
    assert!(dir.join("rust/src/lib.rs").exists(), "missing rust lib.rs");
    assert!(dir.join("go/service.go").exists(), "missing go service.go");
    assert!(dir.join("go/go.mod").exists(), "missing go.mod");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn eject_writes_manifest_and_project() {
    let dir = std::env::temp_dir().join(format!("telos_cli_eject_{}", std::process::id()));
    let (ok, _, stderr) = run(&[
        "eject",
        "../../examples/microservice.telos",
        "--out-dir",
        dir.to_str().unwrap(),
    ]);
    assert!(ok, "eject should exit 0: {stderr}");
    let manifest = std::fs::read_to_string(dir.join("telos-eject.json")).unwrap();
    assert!(
        manifest.contains("ejected"),
        "manifest should list ejected fns"
    );
    assert!(dir.join("rust/src/lib.rs").exists(), "missing rust lib.rs");
    assert!(dir.join("go/service.go").exists(), "missing go service.go");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- --strict-rt tests ----

#[test]
fn strict_rt_fails_on_real_time_go_conflict() {
    let path = write_temp(
        "telos_cli_strict_rt_conflict.telos",
        "@boundary(real_time, network_io) module Ctrl { func tick(x: Int) { } }",
    );
    let dir = std::env::temp_dir().join(format!("telos_cli_strict_rt_{}", std::process::id()));
    let (ok, _, stderr) = run(&[
        "project",
        path.to_str().unwrap(),
        "--out-dir",
        dir.to_str().unwrap(),
        "--strict-rt",
    ]);
    assert!(
        !ok,
        "project --strict-rt should exit non-zero on real_time+Go conflict"
    );
    assert!(
        stderr.contains("strict-rt"),
        "expected strict-rt error message:\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn strict_rt_passes_without_conflict() {
    let path = write_temp(
        "telos_cli_strict_rt_clean.telos",
        "@boundary(real_time) module Ctrl { func tick(x: Int) { } }",
    );
    let dir =
        std::env::temp_dir().join(format!("telos_cli_strict_rt_clean_{}", std::process::id()));
    let (ok, _, stderr) = run(&[
        "project",
        path.to_str().unwrap(),
        "--out-dir",
        dir.to_str().unwrap(),
        "--strict-rt",
    ]);
    assert!(
        ok,
        "project --strict-rt should pass when no conflict:\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verify_shows_interval_bounded_tag() {
    let (ok, stdout, _) = run(&["verify", "../../examples/interval.telos"]);
    assert!(ok, "verify should exit 0 on interval.telos");
    assert!(
        stdout.contains("[interval-bounded]"),
        "expected [interval-bounded] tag:\n{stdout}"
    );
}

#[test]
fn init_creates_scaffold_file() {
    let path = std::env::temp_dir().join(format!("telos_cli_init_{}.telos", std::process::id()));
    let (ok, stdout, stderr) = run(&[
        "init",
        "--module",
        "Ledger",
        "--out",
        path.to_str().unwrap(),
    ]);
    assert!(ok, "init should exit 0: {stderr}");
    assert!(
        stdout.contains("Scaffolded"),
        "expected scaffold message:\n{stdout}"
    );
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("module Ledger"),
        "generated file should contain module Ledger:\n{content}"
    );
    assert!(
        content.contains("func increment"),
        "generated file should contain a func:\n{content}"
    );
    let _ = std::fs::remove_file(&path);
}

/// All `init --template` options must produce a parseable, verifiable module.
#[test]
fn init_templates_all_verify() {
    for template in [
        "simple",
        "dual-backend",
        "eject",
        "real-time",
        "python-ml",
        "cross-module",
    ] {
        let path = std::env::temp_dir().join(format!(
            "telos_cli_init_{}_{}.telos",
            template,
            std::process::id()
        ));
        let (ok, _, stderr) = run(&[
            "init",
            "--module",
            "Demo",
            "--template",
            template,
            "--out",
            path.to_str().unwrap(),
        ]);
        assert!(ok, "init --template {template} should exit 0: {stderr}");
        let (vok, _, vstderr) = run(&["verify", path.to_str().unwrap()]);
        assert!(
            vok,
            "init --template {template} output should verify:\n{vstderr}"
        );
        let _ = std::fs::remove_file(&path);
    }
}

#[test]
fn init_rejects_unknown_template() {
    let path =
        std::env::temp_dir().join(format!("telos_cli_init_bad_{}.telos", std::process::id()));
    let (ok, _, stderr) = run(&[
        "init",
        "--module",
        "Demo",
        "--template",
        "nope",
        "--out",
        path.to_str().unwrap(),
    ]);
    assert!(!ok, "init should exit non-zero on unknown template");
    assert!(
        stderr.contains("unknown template"),
        "expected unknown-template error:\n{stderr}"
    );
    let _ = std::fs::remove_file(&path);
}

/// `telos new` scaffolds a project directory (module + README) that verifies.
#[test]
fn new_scaffolds_verifiable_project() {
    let dir = std::env::temp_dir().join(format!("telos_cli_new_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (ok, _, stderr) = run(&[
        "new",
        "--name",
        "MyProj",
        "--out-dir",
        dir.to_str().unwrap(),
        "--template",
        "simple",
    ]);
    assert!(ok, "new should exit 0: {stderr}");
    let telos = dir.join("MyProj.telos");
    let readme = dir.join("README.md");
    assert!(telos.exists(), "expected {telos:?}");
    assert!(readme.exists(), "expected {readme:?}");
    let (vok, _, vstderr) = run(&["verify", telos.to_str().unwrap()]);
    assert!(vok, "scaffolded project should verify:\n{vstderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- --json output tests ----

#[test]
fn verify_json_outputs_valid_json() {
    let (ok, stdout, _) = run(&["verify", WALLET, "--json"]);
    assert!(ok, "verify --json should exit 0 on valid module");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout should be valid JSON");
    assert_eq!(v["passed"], true, "passed should be true");
    assert!(
        v["functions"].is_array(),
        "functions should be an array:\n{stdout}"
    );
    let funcs = v["functions"].as_array().unwrap();
    assert!(
        !funcs.is_empty(),
        "should have at least one function result"
    );
    assert!(
        funcs[0]["func_name"].is_string(),
        "func_name should be a string"
    );
    assert!(funcs[0]["checks"].is_array(), "checks should be an array");
}

#[test]
fn verify_json_on_broken_module() {
    let (ok, stdout, _) = run(&["verify", BROKEN, "--json"]);
    assert!(!ok, "verify --json should exit non-zero on broken module");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout should be valid JSON");
    assert_eq!(v["passed"], false, "passed should be false");
    let funcs = v["functions"].as_array().unwrap();
    let has_fail = funcs
        .iter()
        .any(|f| !f["all_passed"].as_bool().unwrap_or(true));
    assert!(
        has_fail,
        "at least one function should have all_passed=false"
    );
}

#[test]
fn build_json_outputs_proof_hash() {
    let dir = std::env::temp_dir().join(format!("telos_cli_build_json_{}", std::process::id()));
    let (ok, stdout, stderr) = run(&[
        "build",
        WALLET,
        "--out-dir",
        dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(ok, "build --json should exit 0:\n{stderr}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout should be valid JSON");
    assert!(v["proof_hash"].is_string(), "proof_hash should be a string");
    assert!(
        v["proof_hash"].as_str().unwrap().starts_with("sha256:"),
        "proof_hash should start with sha256:"
    );
    assert!(v["out_dir"].is_string(), "out_dir should be a string");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verify_manifest_passes_for_intact_build() {
    let dir =
        std::env::temp_dir().join(format!("telos_cli_verify_manifest_{}", std::process::id()));
    // First build to generate the proof manifest.
    let (ok, _, stderr) = run(&["build", WALLET, "--out-dir", dir.to_str().unwrap()]);
    assert!(ok, "build should exit 0:\n{stderr}");
    let manifest_path = dir.join("telos-proof.json");
    assert!(manifest_path.exists(), "telos-proof.json should exist");

    // Now verify-manifest against the same source.
    let (ok, stdout, stderr) = run(&["verify-manifest", manifest_path.to_str().unwrap(), WALLET]);
    assert!(ok, "verify-manifest should exit 0:\n{stderr}");
    assert!(
        stdout.contains("MANIFEST OK"),
        "expected MANIFEST OK:\n{stdout}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fmt_stdout_is_idempotent() {
    // Formatting an already-formatted file should be a no-op: fmt(fmt(x)) == fmt(x).
    let (ok, once, stderr) = run(&["fmt", WALLET, "--stdout"]);
    assert!(ok, "fmt --stdout should exit 0:\n{stderr}");
    assert!(!once.is_empty(), "formatted output should not be empty");

    let path = write_temp(
        &format!("telos_fmt_once_{}.telos", std::process::id()),
        &once,
    );
    let (ok, twice, stderr) = run(&["fmt", path.to_str().unwrap(), "--stdout"]);
    assert!(ok, "fmt --stdout (pass 2) should exit 0:\n{stderr}");
    assert_eq!(once, twice, "formatting should be idempotent");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn fmt_check_reports_changes_needed_then_passes_after_writing() {
    let (_, stdout, stderr) = run(&["fmt", WALLET, "--stdout"]);
    assert!(
        stderr.is_empty(),
        "fmt --stdout should not error:\n{stderr}"
    );
    let path = write_temp(
        &format!("telos_fmt_check_{}.telos", std::process::id()),
        &stdout,
    );

    // Already-canonical file: --check should pass (exit 0) and not modify it.
    let (ok, _, stderr) = run(&["fmt", path.to_str().unwrap(), "--check"]);
    assert!(
        ok,
        "fmt --check on canonical input should exit 0:\n{stderr}"
    );

    // Reformat in place, then re-run --check: still exit 0.
    let (ok, _, stderr) = run(&["fmt", path.to_str().unwrap()]);
    assert!(ok, "fmt (write) should exit 0:\n{stderr}");
    let (ok, _, stderr) = run(&["fmt", path.to_str().unwrap(), "--check"]);
    assert!(ok, "fmt --check after formatting should exit 0:\n{stderr}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn fmt_check_fails_on_malformed_file() {
    let path = write_temp(
        &format!("telos_fmt_malformed_{}.telos", std::process::id()),
        "this is not valid telos source {{{",
    );
    let (ok, _, stderr) = run(&["fmt", path.to_str().unwrap(), "--check"]);
    assert!(!ok, "fmt --check on malformed input should exit non-zero");
    assert!(!stderr.is_empty(), "expected a parse error message");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn doctor_runs_and_reports_json() {
    let (ok, stdout, stderr) = run(&["doctor", "--json"]);
    assert!(ok, "doctor --json should exit 0:\n{stderr}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout should be valid JSON");
    let checks = v.as_array().expect("doctor --json should output an array");
    let tools: Vec<&str> = checks.iter().map(|c| c["tool"].as_str().unwrap()).collect();
    assert!(tools.contains(&"go"));
    assert!(tools.contains(&"gofmt"));
    assert!(tools.contains(&"z3"));
    for c in checks {
        assert!(c["found"].is_boolean(), "found should be a bool: {c}");
    }
}

#[test]
fn doctor_runs_human_readable() {
    let (ok, stdout, stderr) = run(&["doctor"]);
    assert!(ok, "doctor should exit 0:\n{stderr}");
    assert!(
        stdout.contains("go"),
        "expected `go` in doctor output:\n{stdout}"
    );
    assert!(
        stdout.contains("gofmt"),
        "expected `gofmt` in doctor output:\n{stdout}"
    );
    assert!(
        stdout.contains("z3"),
        "expected `z3` in doctor output:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Phase 17: equality-substitution example fixtures
// ---------------------------------------------------------------------------

const LEDGER: &str = "../../examples/ledger.telos";
const RATE_LIMITER: &str = "../../examples/rate_limiter.telos";
const QUOTA: &str = "../../examples/quota.telos";

#[test]
fn verify_ledger_passes() {
    let (ok, stdout, _) = run(&["verify", LEDGER]);
    assert!(ok, "ledger.telos should verify cleanly:\n{stdout}");
    assert!(
        stdout.contains("all constraints satisfied"),
        "expected success summary:\n{stdout}"
    );
}

#[test]
fn verify_rate_limiter_passes() {
    let (ok, stdout, _) = run(&["verify", RATE_LIMITER]);
    assert!(ok, "rate_limiter.telos should verify cleanly:\n{stdout}");
    assert!(
        stdout.contains("all constraints satisfied"),
        "expected success summary:\n{stdout}"
    );
}

#[test]
fn verify_quota_passes() {
    let (ok, stdout, _) = run(&["verify", QUOTA]);
    assert!(ok, "quota.telos should verify cleanly:\n{stdout}");
    assert!(
        stdout.contains("all constraints satisfied"),
        "expected success summary:\n{stdout}"
    );
}
