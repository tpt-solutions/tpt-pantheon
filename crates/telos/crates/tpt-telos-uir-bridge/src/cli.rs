//! CLI entry point logic for `telos-uir-prove`.
//!
//! Kept in the library (rather than only in `src/bin`) so integration tests can
//! exercise it in-process. The thin `main()` in `src/bin/telos-uir-prove.rs`
//! forwards `std::env::args()` here and exits with the returned code.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::{prove_tptuir_file, MemoryLimits, ProofResult};

enum CliError {
    Usage(String),
    Help,
}

fn print_help() {
    println!(
        "telos-uir-prove - prove TPT-UIR memory-allocation bounds\n\n\
         Usage: telos-uir-prove <model.tptuir> [--default-limit BYTES] [--scope NAME BYTES]...\n\n\
         Exit codes: 0 = Valid, 1 = Counterexample (over budget), 2 = Inconclusive / error"
    );
}

/// Run the CLI with the given argument iterator. Returns the process exit code:
/// `0` = Valid, `1` = Counterexample (over budget), `2` = Inconclusive / error.
pub fn run_cli<I, T>(args: I) -> u8
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let collected: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();
    match parse_args_with(&collected) {
        Ok(code) => code,
        Err(CliError::Help) => {
            print_help();
            0
        }
        Err(CliError::Usage(e)) => {
            eprintln!("error: {e}");
            2
        }
    }
}

fn parse_args_with(args: &[String]) -> Result<u8, CliError> {
    let mut path: Option<PathBuf> = None;
    let mut default_limit: Option<i64> = None;
    let mut scopes: Vec<(String, i64)> = Vec::new();

    let mut it = args.iter().skip(1).cloned();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--default-limit" => {
                let v = it
                    .next()
                    .ok_or_else(|| CliError::Usage("--default-limit requires a value".into()))?
                    .parse::<i64>()
                    .map_err(|_| CliError::Usage("invalid --default-limit value".into()))?;
                default_limit = Some(v);
            }
            "--scope" => {
                let name = it
                    .next()
                    .ok_or_else(|| CliError::Usage("--scope requires a name".into()))?;
                let v = it
                    .next()
                    .ok_or_else(|| CliError::Usage("--scope requires a byte limit".into()))?
                    .parse::<i64>()
                    .map_err(|_| CliError::Usage("invalid --scope byte limit".into()))?;
                scopes.push((name, v));
            }
            "-h" | "--help" => return Err(CliError::Help),
            other => {
                if other.starts_with('-') {
                    return Err(CliError::Usage(format!("unknown flag: {other}")));
                }
                if path.is_some() {
                    return Err(CliError::Usage("multiple input files given".into()));
                }
                path = Some(PathBuf::from(other));
            }
        }
    }

    let path = path.ok_or_else(|| CliError::Usage("missing input .tptuir file".into()))?;

    let mut per_scope: HashMap<String, i64> = HashMap::new();
    for (name, bytes) in &scopes {
        per_scope.insert(name.clone(), *bytes);
    }
    let limits = MemoryLimits {
        default: default_limit.unwrap_or(i64::MAX),
        per_scope,
    };

    match prove_tptuir_file(&path, &limits) {
        Ok(ProofResult::Valid) => {
            println!("RESULT: valid — every memory scope stays within budget");
            Ok(0)
        }
        Ok(ProofResult::Counterexample {
            scope,
            model,
            total_bytes,
            limit_bytes,
        }) => {
            println!("RESULT: counterexample — scope '{scope}' can exceed its budget");
            println!("  total_bytes = {total_bytes}");
            println!("  limit_bytes = {limit_bytes}");
            println!("  witness:");
            let mut keys: Vec<&String> = model.keys().collect();
            keys.sort();
            for k in keys {
                println!("    {k} = {}", model[k]);
            }
            Ok(1)
        }
        Ok(ProofResult::Inconclusive { reason }) => {
            println!("RESULT: inconclusive — {reason}");
            Ok(2)
        }
        Err(e) => {
            eprintln!("error: failed to read {}: {e}", path.display());
            Ok(2)
        }
    }
}
