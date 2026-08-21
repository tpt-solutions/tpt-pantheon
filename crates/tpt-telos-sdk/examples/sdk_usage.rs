//! Example: driving tpt-telos programmatically via `tpt-telos-sdk`.
//!
//! This is a runnable documentation example (it lives under `examples/` and is
//! referenced from the root README). It shows the one-call pipeline
//! (`compile`), how to read verification outcomes, and how to render
//! counterexample hints for any function that failed to verify.
//!
//! Run it with:
//!
//! ```sh
//! cargo run -p tpt-telos-sdk --example sdk_usage
//! ```

use tpt_telos_sdk::{compile, StaticAgent};

fn main() {
    let src = r#"
        module Bank {
            invariant Wallet { balance >= 0 }
            func deposit(w: Wallet, amount: Int)
                requires amount > 0
                ensures w.balance == old(w.balance) + amount
            {
                mutate state { w.balance += amount }
            }
        }
    "#;

    // One call: parse -> agentic transpile -> codegen -> proof manifest.
    let artifact = compile(src, &StaticAgent::new())
        .expect("the pipeline (parse/transpile/codegen) should run");

    println!("all_verified = {}", artifact.all_verified);
    println!("functions   = {}", artifact.outcomes.len());
    println!("project has rust = {}", artifact.project.has_rust);
    println!("project has go   = {}", artifact.project.has_go);
    println!("proof manifest hash = {}", artifact.manifest.manifest_hash);

    // Render human/LLM-readable hints for the verification outcome of each function.
    for outcome in &artifact.outcomes {
        let hint = tpt_telos_sdk::format_outcome_hints(outcome);
        println!("hint: {hint}");
    }

    assert!(artifact.all_verified, "deposit must verify");
}
