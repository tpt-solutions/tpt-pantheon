use serde_json::{json, Value};
use tpt_telos_lsp::{analysis, Server};

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn diagnostics_pass_for_verified_wallet() {
    let text = read("../../examples/wallet.telos");
    let diags = analysis::diagnostics(&text);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

#[test]
fn diagnostics_flag_broken_contract_on_func_line() {
    let text = read("../../examples/broken.telos");
    let diags = analysis::diagnostics(&text);
    assert!(
        !diags.is_empty(),
        "expected a diagnostic for broken contract"
    );
    let d = &diags[0];
    assert_eq!(d.severity, analysis::SEVERITY_ERROR);
    // `func transfer(...)` is on line 8 (0-based 7).
    assert_eq!(d.line, 7, "diagnostic should point at the func line");
    assert!(d.message.contains("contract not satisfied"));
}

#[test]
fn ejected_function_is_informational_not_error() {
    // A wrong body that is ejected must not raise an error diagnostic; the
    // eject hatch trusts the opaque block and guards it at the boundary.
    let text = r#"
module M {
    invariant W { balance >= 0 }
    @eject(rust)
    func f(w: W, amount: PositiveInt)
        requires w.balance >= amount
        ensures w.balance == old(w.balance) - amount
    {
        mutate state {
            w.balance += amount
        }
    }
}
"#;
    let diags = analysis::diagnostics(text);
    assert!(
        diags.iter().all(|d| d.severity != analysis::SEVERITY_ERROR),
        "ejected function should not produce error diagnostics: {diags:?}"
    );
}

#[test]
fn hover_reports_contract_and_status() {
    let text = read("../../examples/wallet.telos");
    // Hover over `transfer` on its definition line (line index 16 in the file).
    let md = analysis::hover_markdown(&text, 16, 9).expect("hover over transfer");
    assert!(md.contains("func `transfer`"));
    assert!(md.contains("VERIFIED"));
    assert!(md.contains("requires"));
    assert!(md.contains("old(from.balance) - amount"));
}

#[test]
fn server_initialize_advertises_capabilities() {
    let mut s = Server::new();
    let out = s.handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }));
    assert_eq!(out.len(), 1);
    let caps = &out[0]["result"]["capabilities"];
    assert_eq!(caps["hoverProvider"], json!(true));
    assert_eq!(caps["textDocumentSync"], json!(1));
}

#[test]
fn did_open_publishes_diagnostics() {
    let mut s = Server::new();
    let text = read("../../examples/broken.telos");
    let out = s.handle(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": { "textDocument": { "uri": "file:///broken.telos", "text": text } }
    }));
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["method"], json!("textDocument/publishDiagnostics"));
    let diags = out[0]["params"]["diagnostics"].as_array().unwrap();
    assert!(!diags.is_empty());
    assert_eq!(diags[0]["source"], json!("telos"));
}

#[test]
fn verify_and_eject_custom_requests() {
    let mut s = Server::new();
    let text = read("../../examples/microservice.telos");
    let uri = "file:///microservice.telos";
    s.handle(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": { "textDocument": { "uri": uri, "text": text } }
    }));

    // telos/verify
    let v = s.handle(&json!({
        "jsonrpc": "2.0", "id": 2, "method": "telos/verify",
        "params": { "uri": uri }
    }));
    assert_eq!(v[0]["result"]["ok"], json!(true));
    let funcs = v[0]["result"]["functions"].as_array().unwrap();
    assert_eq!(funcs.len(), 4);

    // telos/eject
    let e = s.handle(&json!({
        "jsonrpc": "2.0", "id": 3, "method": "telos/eject",
        "params": { "uri": uri }
    }));
    let preview: &Value = &e[0]["result"]["preview"];
    let preview = preview.as_str().unwrap();
    assert!(preview.contains("settle_impl"));
    assert!(preview.contains("Boundary contract guard"));
    assert!(preview.contains("enqueueImpl"));
}

#[test]
fn unknown_request_returns_method_not_found() {
    let mut s = Server::new();
    let out = s.handle(&json!({ "jsonrpc": "2.0", "id": 9, "method": "no/such/method" }));
    assert_eq!(out[0]["error"]["code"], json!(-32601));
}

#[test]
fn shutdown_then_exit_flags_server() {
    let mut s = Server::new();
    let out = s.handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "shutdown" }));
    assert_eq!(out[0]["result"], Value::Null);
    assert!(!s.should_exit());
    s.handle(&json!({ "jsonrpc": "2.0", "method": "exit" }));
    assert!(s.should_exit());
}
