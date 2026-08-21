use tpt_telos_agent::{transpile_module, StaticAgent};
use tpt_telos_parser::parse;

fn outcomes_for(path: &str) -> Vec<tpt_telos_agent::FuncOutcome> {
    let src = std::fs::read_to_string(path).unwrap();
    let modules = parse(&src).unwrap();
    let agent = StaticAgent::new();
    let mut all = Vec::new();
    for m in &modules {
        all.extend(transpile_module(m, &agent).unwrap());
    }
    all
}

#[test]
fn wallet_is_verified() {
    let outs = outcomes_for("../../examples/wallet.telos");
    assert_eq!(outs.len(), 1);
    assert!(outs[0].verified, "wallet transfer must verify");
    assert!(!outs[0].iterations.is_empty());
}

#[test]
fn intent_only_is_synthesized_and_verified() {
    let outs = outcomes_for("../../examples/intent.telos");
    assert_eq!(outs.len(), 2);
    for o in &outs {
        assert!(
            o.verified,
            "{} should be synthesized and verified",
            o.func_name
        );
    }
}

#[test]
fn broken_is_repaired_by_the_loop() {
    let outs = outcomes_for("../../examples/broken.telos");
    assert_eq!(outs.len(), 1);
    // The first candidate (the wrong user body) must fail, then the loop must
    // repair it into a verified implementation.
    assert!(
        outs[0].iterations.len() >= 2,
        "expected generate + rewrite steps"
    );
    assert!(!outs[0].iterations[0].passed, "first candidate should fail");
    assert!(
        outs[0].verified,
        "loop must end in a verified implementation"
    );

    let text = tpt_telos_agent::render_candidate(&outs[0].final_candidate);
    assert!(
        text.contains("from.balance - amount"),
        "repaired body should subtract: {}",
        text
    );
    assert!(
        !text.contains("from.balance + amount"),
        "repaired body must not add: {}",
        text
    );
}
