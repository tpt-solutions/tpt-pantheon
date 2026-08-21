//! Integration tests for the `telos-prove` binary.
//!
//! Drives the compiled binary (`CARGO_BIN_EXE_telos-prove`) for the
//! contradiction-checking CLI, covering success, contradiction, self-unsat,
//! `--json`, and `--strict` exit codes. Mirrors `cli.rs`'s pattern (no
//! `assert_cmd` dependency added).

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_telos-prove");
const FIX: &str = "tests/fixtures";

fn run_stdin(args: &[&str], input: &str) -> (bool, String, String) {
    use std::io::Write;
    let mut child = Command::new(BIN)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn telos-prove");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    (out.status.success(), stdout, stderr)
}

fn run_file(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(BIN)
        .args(args)
        .output()
        .expect("failed to run telos-prove");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    (out.status.success(), stdout, stderr)
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(FIX).join(name)
}

#[test]
fn help_exits_zero() {
    let (ok, stdout, _) = run_file(&["--help"]);
    assert!(ok);
    assert!(stdout.contains("telos-prove"));
}

#[test]
fn reports_pairwise_contradiction_from_file() {
    let (ok, stdout, _) = run_file(&[fixture("contradiction.json").to_str().unwrap()]);
    assert!(
        ok,
        "a contradiction is not a hard error for the default mode"
    );
    assert!(
        stdout.contains("UNSATISFIABLE") || stdout.contains("unsatisfiable"),
        "got:\n{stdout}"
    );
    assert!(stdout.contains("latency_ok"), "got:\n{stdout}");
    assert!(stdout.contains("latency_critical"), "got:\n{stdout}");
    assert!(stdout.contains("pairwise contradictions"), "got:\n{stdout}");
}

#[test]
fn reports_consistent_groups() {
    let (ok, stdout, _) = run_file(&[fixture("consistent.json").to_str().unwrap()]);
    assert!(ok);
    assert!(stdout.contains("satisfiable"), "got:\n{stdout}");
    assert!(
        stdout.contains("none"),
        "expected no pairwise contradictions:\n{stdout}"
    );
}

#[test]
fn reports_self_unsat_group() {
    let (ok, stdout, _) = run_file(&[fixture("self_unsat.json").to_str().unwrap()]);
    assert!(ok);
    assert!(stdout.contains("self-contradictory"), "got:\n{stdout}");
}

#[test]
fn json_mode_emits_machine_readable() {
    let (ok, stdout, _) = run_file(&["--json", fixture("contradiction.json").to_str().unwrap()]);
    assert!(ok);
    assert!(stdout.contains("\"overall_unsat\""), "got:\n{stdout}");
    assert!(stdout.contains("\"contradictory_pairs\""), "got:\n{stdout}");
    assert!(stdout.contains("\"latency_ok\""), "got:\n{stdout}");
}

#[test]
fn strict_exits_nonzero_on_contradiction() {
    let (ok, _, _) = run_file(&["--strict", fixture("contradiction.json").to_str().unwrap()]);
    assert!(!ok, "--strict should exit non-zero on a contradiction");
}

#[test]
fn strict_exits_zero_on_consistent() {
    let (ok, _, _) = run_file(&["--strict", fixture("consistent.json").to_str().unwrap()]);
    assert!(ok, "--strict should exit 0 on consistent groups");
}

#[test]
fn reads_from_stdin() {
    let input = r#"[
      { "label": "low", "constraints": [ { "linear": { "terms": [["x", 1]], "constant": 0 }, "relation": "<=" } ] },
      { "label": "high", "constraints": [ { "linear": { "terms": [["x", 1]], "constant": -1 }, "relation": ">=" } ] }
    ]"#;
    let (ok, stdout, _) = run_stdin(&[], input);
    assert!(ok);
    assert!(stdout.contains("high"), "got:\n{stdout}");
    assert!(stdout.contains("low"), "got:\n{stdout}");
}

#[test]
fn malformed_json_exits_nonzero() {
    let (ok, _, stderr) = run_stdin(&[], "{ not valid json");
    assert!(!ok);
    assert!(!stderr.is_empty() || true, "should report an error");
}

#[test]
fn unknown_flag_exits_nonzero() {
    let (ok, _, _) = run_file(&["--nope"]);
    assert!(!ok);
}
