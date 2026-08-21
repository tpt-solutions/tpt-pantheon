//! `telos-prove` — a thin CLI over `tpt-telos-sdk`'s constraint-group
//! contradiction checker.
//!
//! Reads named constraint groups as JSON (from a file argument or stdin) and
//! reports pairwise/joint contradictions via the self-contained FM solver. This
//! is for callers with no `.telos` source who want a standalone binary rather
//! than a library dependency (see [`tpt_telos_sdk::check_contradictions`]).
//!
//! Usage:
//!   telos-prove [FILE] [--json] [--strict]
//!   telos-prove --help
//!
//! `FILE` defaults to stdin when omitted. `--json` emits machine-readable
//! JSON; `--strict` exits non-zero whenever any contradiction (self- or
//! pairwise) or undecided result is found.

use std::io::Read;
use std::process::ExitCode;

use tpt_telos_sdk::json::JsonError;
use tpt_telos_sdk::prove::{build_report, parse_groups};

const HELP: &str = "\
telos-prove — contradiction-check named constraint groups (standalone binary over the tpt-telos solver core)

USAGE:
    telos-prove [FILE] [--json] [--strict]
    telos-prove --help

ARGS:
    FILE            Path to a JSON file of constraint groups. Defaults to stdin
                    when omitted.

OPTIONS:
    --json          Emit machine-readable JSON instead of human-readable text.
    --strict        Exit non-zero when any contradiction (self- or pairwise) or
                    an undecided result is found.
    --help          Print this help text.

INPUT SCHEMA (top-level array of groups, or an object with a \"groups\" array):
    {
      \"groups\": [
        {
          \"label\": \"latency_ok\",
          \"constraints\": [
            { \"linear\": { \"terms\": [[\"value\", 1]], \"constant\": -200 }, \"relation\": \"<=\" }
          ]
        }
      ]
    }

  relation is one of <= >= == < > != . A linear's terms are [variable, coefficient]
  pairs; constant is added directly, so [\"value\",1] with constant -200 and \"<=\"
  encodes value <= 200.
";

fn main() -> ExitCode {
    let args = std::env::args().skip(1);
    let mut file: Option<String> = None;
    let mut json = false;
    let mut strict = false;
    let mut show_help = false;

    for a in args {
        match a.as_str() {
            "--json" => json = true,
            "--strict" => strict = true,
            "--help" | "-h" => show_help = true,
            other if other.starts_with("--") => {
                eprintln!("error: unknown flag {other}");
                eprintln!("{HELP}");
                return ExitCode::FAILURE;
            }
            other => {
                if file.is_some() {
                    eprintln!("error: unexpected extra argument {other}");
                    eprintln!("{HELP}");
                    return ExitCode::FAILURE;
                }
                file = Some(other.to_string());
            }
        }
    }

    if show_help {
        print!("{HELP}");
        return ExitCode::SUCCESS;
    }

    let src = match read_input(file.as_deref()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let groups = match parse_groups(&src) {
        Ok(g) => g,
        Err(e) => {
            let msg = match &e {
                JsonError { offset, .. } if !src.is_empty() => e.display(&src),
                _ => format!("error: {}", e.message),
            };
            eprintln!("{msg}");
            return ExitCode::FAILURE;
        }
    };

    if groups.is_empty() {
        eprintln!("error: no constraint groups supplied");
        return ExitCode::FAILURE;
    }

    let report = build_report(&groups);
    if json {
        print!("{}", report.format_json());
    } else {
        print!("{}", report.format_human());
    }

    if strict {
        let any_conflict = report
            .contradictory_pairs
            .iter()
            .any(|(a, b)| !a.is_empty() && !b.is_empty());
        let any_self_unsat = report.groups.iter().any(|g| g.self_unsat == Some(true));
        let any_undecided =
            report.overall_unsat.is_none() || report.groups.iter().any(|g| g.self_unsat.is_none());
        if any_conflict || any_self_unsat || any_undecided {
            return ExitCode::FAILURE;
        }
    }

    ExitCode::SUCCESS
}

fn read_input(file: Option<&str>) -> Result<String, String> {
    match file {
        Some(path) => {
            std::fs::read_to_string(path).map_err(|e| format!("could not read {path}: {e}"))
        }
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| format!("could not read stdin: {e}"))?;
            Ok(buf)
        }
    }
}
