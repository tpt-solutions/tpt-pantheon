//! Language Server Protocol server for tpt-telos (Phase 4).
//!
//! Provides real-time, in-editor feedback for `.telos` files:
//!   * **diagnostics** -- parse errors and unsatisfied mathematical contracts,
//!     published on open/change/save (ejected functions are reported as trusted),
//!   * **hover** -- a function's signature, routing target, contract, and
//!     verification status,
//!   * **custom requests** -- `telos/verify` (a verification summary) and
//!     `telos/eject` (a preview of the ejected raw Rust/Go with contract guards).
//!
//! The protocol layer speaks JSON-RPC 2.0 over stdio with `Content-Length`
//! framing. The [`Server`] message handler is decoupled from I/O so it can be
//! unit-tested by feeding it JSON values directly.

pub mod analysis;

use std::collections::HashMap;
use std::io::{BufRead, Write};

use serde_json::{json, Value};

pub use analysis::{
    analyze, build_index, code_actions, completion_items, definition_at, diagnostics,
    format_source, hover_markdown, inlay_hints, references_at, word_at_pos, Diagnostic, InlayHint,
    Location, QuickFix, SymKind, Symbol, SymbolIndex,
};

/// The language server state: open documents and lifecycle flags.
pub struct Server {
    documents: HashMap<String, String>,
    shutdown: bool,
    exit: bool,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    /// Create a new, idle language server instance.
    ///
    /// # Examples
    ///
    /// ```
    /// use tpt_telos_lsp::Server;
    ///
    /// let server = Server::new();
    /// assert!(!server.should_exit());
    /// ```
    pub fn new() -> Self {
        Server {
            documents: HashMap::new(),
            shutdown: false,
            exit: false,
        }
    }

    /// Whether the client has requested the server to exit.
    ///
    /// # Examples
    ///
    /// ```
    /// use tpt_telos_lsp::Server;
    /// use serde_json::json;
    ///
    /// let mut server = Server::new();
    /// assert!(!server.should_exit());
    ///
    /// server.handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "shutdown" }));
    /// assert!(!server.should_exit()); // shutdown sets internal flag, not exit yet
    ///
    /// server.handle(&json!({ "jsonrpc": "2.0", "method": "exit" }));
    /// assert!(server.should_exit());
    /// ```
    pub fn should_exit(&self) -> bool {
        self.exit
    }

    /// Handle one incoming JSON-RPC message, returning zero or more outgoing
    /// messages (responses and/or notifications) to write back to the client.
    ///
    /// # Examples
    ///
    /// ```
    /// use tpt_telos_lsp::Server;
    /// use serde_json::json;
    ///
    /// let mut server = Server::new();
    ///
    /// // Initialize: the server responds with its capability advertisement.
    /// let responses = server.handle(&json!({
    ///     "jsonrpc": "2.0",
    ///     "id": 1,
    ///     "method": "initialize",
    ///     "params": {}
    /// }));
    /// assert_eq!(responses.len(), 1);
    /// assert!(responses[0]["result"]["capabilities"]["hoverProvider"].as_bool().unwrap());
    ///
    /// // Open a document: the server publishes diagnostics.
    /// let responses = server.handle(&json!({
    ///     "jsonrpc": "2.0",
    ///     "method": "textDocument/didOpen",
    ///     "params": {
    ///         "textDocument": {
    ///             "uri": "file:///test.telos",
    ///             "text": "module M {}"
    ///         }
    ///     }
    /// }));
    /// assert_eq!(responses[0]["method"], "textDocument/publishDiagnostics");
    /// ```
    pub fn handle(&mut self, msg: &Value) -> Vec<Value> {
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let id = msg.get("id").cloned();

        match method {
            "initialize" => vec![response(
                id,
                json!({
                    "capabilities": {
                        "textDocumentSync": 1,
                        "hoverProvider": true,
                        "codeActionProvider": true,
                        "documentFormattingProvider": true,
                        "definitionProvider": true,
                        "referencesProvider": true,
                        "completionProvider": { "triggerCharacters": [".", "("] },
                        "inlayHintProvider": true
                    },
                    "serverInfo": { "name": "telos-lsp", "version": env!("CARGO_PKG_VERSION") }
                }),
            )],
            "initialized" => vec![],
            "shutdown" => {
                self.shutdown = true;
                vec![response(id, Value::Null)]
            }
            "exit" => {
                self.exit = true;
                vec![]
            }
            "textDocument/didOpen" => {
                let uri = msg["params"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let text = msg["params"]["textDocument"]["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                self.documents.insert(uri.clone(), text);
                vec![self.publish(&uri)]
            }
            "textDocument/didChange" => {
                let uri = msg["params"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                if let Some(changes) = msg["params"]["contentChanges"].as_array() {
                    if let Some(last) = changes.last() {
                        if let Some(text) = last["text"].as_str() {
                            self.documents.insert(uri.clone(), text.to_string());
                        }
                    }
                }
                vec![self.publish(&uri)]
            }
            "textDocument/didSave" => {
                let uri = msg["params"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                vec![self.publish(&uri)]
            }
            "textDocument/didClose" => {
                let uri = msg["params"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                self.documents.remove(&uri);
                vec![publish_diagnostics(&uri, &[])]
            }
            "textDocument/hover" => {
                let uri = msg["params"]["textDocument"]["uri"].as_str().unwrap_or("");
                let line = msg["params"]["position"]["line"].as_u64().unwrap_or(0) as usize;
                let ch = msg["params"]["position"]["character"].as_u64().unwrap_or(0) as usize;
                let result = self
                    .documents
                    .get(uri)
                    .and_then(|text| hover_markdown(text, line, ch))
                    .map(|md| json!({ "contents": { "kind": "markdown", "value": md } }))
                    .unwrap_or(Value::Null);
                vec![response(id, result)]
            }
            "textDocument/codeAction" => {
                let uri = msg["params"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let actions: Vec<Value> = self
                    .documents
                    .get(&uri)
                    .map(|text| code_actions(text))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|fix| {
                        json!({
                            "title": fix.title,
                            "kind": "quickfix",
                            "edit": {
                                "changes": {
                                    uri.clone(): [{
                                        "range": {
                                            "start": { "line": fix.line, "character": 0 },
                                            "end": { "line": fix.line, "character": 0 }
                                        },
                                        "newText": fix.new_text
                                    }]
                                }
                            }
                        })
                    })
                    .collect();
                vec![response(id, json!(actions))]
            }
            "textDocument/formatting" => {
                let uri = msg["params"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let result = match self.documents.get(&uri) {
                    Some(text) => match format_source(text) {
                        Ok(formatted) => {
                            let line_count = text.lines().count();
                            let last_line_len = text.lines().last().map(|l| l.len()).unwrap_or(0);
                            json!([{
                                "range": {
                                    "start": { "line": 0, "character": 0 },
                                    "end": {
                                        "line": line_count,
                                        "character": last_line_len
                                    }
                                },
                                "newText": formatted
                            }])
                        }
                        Err(_) => json!([]),
                    },
                    None => json!([]),
                };
                vec![response(id, result)]
            }
            "telos/verify" => {
                let uri = msg["params"]["uri"].as_str().unwrap_or("");
                let result = match self.documents.get(uri) {
                    Some(text) => verify_summary(text),
                    None => json!({ "error": "document not open" }),
                };
                vec![response(id, result)]
            }
            "telos/eject" => {
                let uri = msg["params"]["uri"].as_str().unwrap_or("");
                let func = msg["params"]["func"].as_str();
                let result = match self.documents.get(uri) {
                    Some(text) => match eject_preview(text, func) {
                        Ok(preview) => json!({ "preview": preview }),
                        Err(e) => json!({ "error": e }),
                    },
                    None => json!({ "error": "document not open" }),
                };
                vec![response(id, result)]
            }
            "textDocument/definition" => {
                let uri = msg["params"]["textDocument"]["uri"].as_str().unwrap_or("");
                let line = msg["params"]["position"]["line"].as_u64().unwrap_or(0) as usize;
                let ch = msg["params"]["position"]["character"].as_u64().unwrap_or(0) as usize;
                let docs = self.index_docs();
                let out = self
                    .documents
                    .get(uri)
                    .and_then(|text| word_at_pos(text, line, ch))
                    .and_then(|w| {
                        let idx = build_index(&docs);
                        definition_at(&idx, &w)
                    })
                    .map(location_to_json)
                    .unwrap_or(Value::Null);
                vec![response(id, out)]
            }
            "textDocument/references" => {
                let uri = msg["params"]["textDocument"]["uri"].as_str().unwrap_or("");
                let line = msg["params"]["position"]["line"].as_u64().unwrap_or(0) as usize;
                let ch = msg["params"]["position"]["character"].as_u64().unwrap_or(0) as usize;
                let docs = self.index_docs();
                let locations = self
                    .documents
                    .get(uri)
                    .and_then(|text| word_at_pos(text, line, ch))
                    .map(|w| {
                        let idx = build_index(&docs);
                        references_at(&idx, &w)
                            .into_iter()
                            .map(location_to_json)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                vec![response(id, json!(locations))]
            }
            "textDocument/completion" => {
                let uri = msg["params"]["textDocument"]["uri"].as_str().unwrap_or("");
                let items = match self.documents.get(uri) {
                    Some(_) => {
                        let docs = self.index_docs();
                        let idx = build_index(&docs);
                        completion_items(&idx)
                            .into_iter()
                            .map(|(label, kind)| json!({ "label": label, "kind": kind }))
                            .collect::<Vec<_>>()
                    }
                    None => Vec::new(),
                };
                vec![response(id, json!(items))]
            }
            "textDocument/inlayHint" => {
                let uri = msg["params"]["textDocument"]["uri"].as_str().unwrap_or("");
                let hints = self
                    .documents
                    .get(uri)
                    .map(|text| inlay_hints(text, uri))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|h| {
                        json!({
                            "position": { "line": h.line, "character": h.character },
                            "label": h.label,
                            "kind": h.kind,
                            "paddingLeft": true
                        })
                    })
                    .collect::<Vec<_>>();
                vec![response(id, json!(hints))]
            }
            _ => {
                if id.is_some() {
                    vec![error_response(
                        id,
                        -32601,
                        &format!("method not found: {method}"),
                    )]
                } else {
                    vec![]
                }
            }
        }
    }

    fn publish(&self, uri: &str) -> Value {
        let diags = self
            .documents
            .get(uri)
            .map(|text| diagnostics(text))
            .unwrap_or_default();
        publish_diagnostics(uri, &diags)
    }

    /// Snapshot of all open documents as `(uri, text)` pairs, for the workspace
    /// symbol index.
    fn index_docs(&self) -> Vec<(String, String)> {
        self.documents
            .iter()
            .map(|(u, t)| (u.clone(), t.clone()))
            .collect()
    }
}

fn verify_summary(text: &str) -> Value {
    match analyze(text) {
        Err(e) => json!({ "ok": false, "error": e }),
        Ok(reports) => {
            let funcs: Vec<Value> = reports
                .iter()
                .map(|r| {
                    json!({
                        "module": r.module,
                        "func": r.name,
                        "target": r.target,
                        "ejected": r.ejected,
                        "verified": r.verified,
                        "failures": r.failures,
                    })
                })
                .collect();
            let all = reports.iter().all(|r| r.verified || r.ejected);
            json!({ "ok": all, "functions": funcs })
        }
    }
}

fn eject_preview(text: &str, func: Option<&str>) -> Result<String, String> {
    use tpt_telos_parser::ast::{Arg, Attribute, Item};

    let mut modules = tpt_telos_parser::parse(text)?;
    let agent = tpt_telos_agent::StaticAgent::new();
    let mut outcomes = Vec::new();
    for m in &modules {
        outcomes.extend(tpt_telos_agent::transpile_module(m, &agent)?);
    }

    let mut matched = false;
    for m in &mut modules {
        let lang = tpt_telos_router::route(&m.attributes)
            .target
            .as_str()
            .to_string();
        for item in &mut m.items {
            if let Item::Func(f) = item {
                let selected = func.map(|n| n == f.name).unwrap_or(true);
                if selected && !f.is_ejected() {
                    f.attributes.push(Attribute {
                        name: "eject".to_string(),
                        args: vec![Arg::Flag(lang.clone())],
                    });
                }
                if selected {
                    matched = true;
                }
            }
        }
    }
    if !matched {
        return Err(format!(
            "no matching function to eject{}",
            func.map(|n| format!(" (`{n}`)")).unwrap_or_default()
        ));
    }

    let project = tpt_telos_codegen::generate_project(&modules, &outcomes)?;
    let mut preview = String::new();
    for f in &project.files {
        if f.path.ends_with(".rs") || f.path.ends_with(".go") {
            preview.push_str(&format!("// ===== {} =====\n", f.path));
            preview.push_str(&f.contents);
            preview.push('\n');
        }
    }
    Ok(preview)
}

fn response(id: Option<Value>, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id.unwrap_or(Value::Null), "result": result })
}

fn error_response(id: Option<Value>, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id.unwrap_or(Value::Null), "error": { "code": code, "message": message } })
}

fn publish_diagnostics(uri: &str, diags: &[Diagnostic]) -> Value {
    let items: Vec<Value> = diags
        .iter()
        .map(|d| {
            json!({
                "range": {
                    "start": { "line": d.line, "character": d.character },
                    "end": { "line": d.end_line, "character": d.end_character }
                },
                "severity": d.severity,
                "source": "telos",
                "message": d.message,
            })
        })
        .collect();
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": { "uri": uri, "diagnostics": items }
    })
}

fn location_to_json(loc: Location) -> Value {
    json!({
        "uri": loc.uri,
        "range": {
            "start": { "line": loc.line, "character": loc.character },
            "end": { "line": loc.end_line, "character": loc.end_character }
        }
    })
}

// ------------------------------------------------------------- stdio loop

/// Run the language server over stdio until the client sends `exit`.
///
/// # Examples
///
/// `run_stdio` is designed to be the entry point of a long-running LSP server
/// process. In production it is called from `main`:
///
/// ```no_run
/// tpt_telos_lsp::run_stdio().unwrap();
/// ```
///
/// For programmatic use without a full stdio loop, drive [`Server`] directly:
///
/// ```
/// use tpt_telos_lsp::Server;
/// use serde_json::json;
///
/// let mut server = Server::new();
/// let _ = server.handle(&json!({
///     "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}
/// }));
/// ```
pub fn run_stdio() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    let mut server = Server::new();

    while let Some(msg) = read_message(&mut reader)? {
        for out in server.handle(&msg) {
            write_message(&mut writer, &out)?;
        }
        if server.should_exit() {
            break;
        }
    }
    Ok(())
}

/// Read one `Content-Length`-framed JSON-RPC message from `reader`.
fn read_message<R: BufRead>(reader: &mut R) -> std::io::Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut header = String::new();
        let n = reader.read_line(&mut header)?;
        if n == 0 {
            return Ok(None); // EOF
        }
        let trimmed = header.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break; // end of headers
        }
        if let Some(rest) = trimmed
            .strip_prefix("Content-Length:")
            .or_else(|| trimmed.strip_prefix("content-length:"))
        {
            content_length = rest.trim().parse::<usize>().ok();
        }
    }

    let len = match content_length {
        Some(l) => l,
        None => return Ok(None),
    };

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    match serde_json::from_slice::<Value>(&buf) {
        Ok(v) => Ok(Some(v)),
        Err(_) => Ok(Some(Value::Null)),
    }
}

/// Write one `Content-Length`-framed JSON-RPC message to `writer`.
fn write_message<W: Write>(writer: &mut W, msg: &Value) -> std::io::Result<()> {
    let body = serde_json::to_string(msg).unwrap_or_else(|_| "null".to_string());
    write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn framing_round_trips() {
        let msg = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" });
        let mut buf = Vec::new();
        write_message(&mut buf, &msg).unwrap();

        let framed = String::from_utf8(buf.clone()).unwrap();
        assert!(framed.starts_with("Content-Length: "));
        assert!(framed.contains("\r\n\r\n"));

        let mut reader = Cursor::new(buf);
        let back = read_message(&mut reader).unwrap().unwrap();
        assert_eq!(back, msg);
        // Second read hits EOF.
        assert!(read_message(&mut reader).unwrap().is_none());
    }

    #[test]
    fn handle_code_action_suggests_requires_fix() {
        let mut s = Server::new();
        let src = r#"
            module Bank {
                invariant Wallet { balance >= 0 }
                func withdraw(w: Wallet, amount: Int)
                    ensures w.balance == old(w.balance) - amount
                { mutate state { w.balance -= amount } }
            }
        "#;
        s.handle(&json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": { "textDocument": { "uri": "u", "text": src } }
        }));
        let out = s.handle(&json!({
            "jsonrpc": "2.0", "id": 2, "method": "textDocument/codeAction",
            "params": { "textDocument": { "uri": "u" } }
        }));
        let actions = out[0]["result"].as_array().unwrap();
        assert!(!actions.is_empty());
        assert_eq!(actions[0]["kind"], json!("quickfix"));
        assert!(actions[0]["edit"]["changes"]["u"][0]["newText"]
            .as_str()
            .unwrap()
            .contains("requires"));
    }

    #[test]
    fn handle_did_change_updates_document() {
        let mut s = Server::new();
        s.handle(&json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": { "textDocument": { "uri": "u", "text": "module M {}" } }
        }));
        let out = s.handle(&json!({
            "jsonrpc": "2.0", "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": "u" },
                "contentChanges": [ { "text": "module M { invariant W { balance >= 0 } }" } ]
            }
        }));
        assert_eq!(out[0]["method"], json!("textDocument/publishDiagnostics"));
    }

    #[test]
    fn handle_definition_resolves_to_func() {
        let mut s = Server::new();
        let src = "module M {\n    func f() ;\n    func g() { f() }\n}";
        s.handle(&json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": { "textDocument": { "uri": "u", "text": src } }
        }));
        // Cursor on the call `f()` (line 2, column 15).
        let out = s.handle(&json!({
            "jsonrpc": "2.0", "id": 9, "method": "textDocument/definition",
            "params": { "textDocument": { "uri": "u" }, "position": { "line": 2, "character": 15 } }
        }));
        let loc = &out[0]["result"];
        assert_eq!(loc["uri"], json!("u"));
        // Definition is on line 1 (0-based) where `func f` is declared.
        assert_eq!(loc["range"]["start"]["line"], json!(1));
    }

    #[test]
    fn handle_references_finds_call_site() {
        let mut s = Server::new();
        let src = "module M {\n    func f() ;\n    func g() { f() }\n}";
        s.handle(&json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": { "textDocument": { "uri": "u", "text": src } }
        }));
        let out = s.handle(&json!({
            "jsonrpc": "2.0", "id": 10, "method": "textDocument/references",
            "params": { "textDocument": { "uri": "u" }, "position": { "line": 1, "character": 9 } }
        }));
        let locs = out[0]["result"].as_array().unwrap();
        // definition + the call site.
        assert_eq!(locs.len(), 2);
        assert!(locs.iter().any(|l| l["range"]["start"]["line"] == json!(2)));
    }

    #[test]
    fn handle_completion_lists_symbols() {
        let mut s = Server::new();
        let src = "module M {\n    func f() ;\n    func g() { f() }\n}";
        s.handle(&json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": { "textDocument": { "uri": "u", "text": src } }
        }));
        let out = s.handle(&json!({
            "jsonrpc": "2.0", "id": 11, "method": "textDocument/completion",
            "params": { "textDocument": { "uri": "u" } }
        }));
        let items = out[0]["result"].as_array().unwrap();
        let labels: Vec<&str> = items.iter().map(|i| i["label"].as_str().unwrap()).collect();
        assert!(labels.contains(&"f"));
        assert!(labels.contains(&"g"));
    }

    #[test]
    fn handle_inlay_hints_shows_routing_target() {
        let mut s = Server::new();
        let src = "module M {\n    func f() ;\n}";
        s.handle(&json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": { "textDocument": { "uri": "u", "text": src } }
        }));
        let out = s.handle(&json!({
            "jsonrpc": "2.0", "id": 12, "method": "textDocument/inlayHint",
            "params": { "textDocument": { "uri": "u" }, "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 2, "character": 0 } } }
        }));
        let hints = out[0]["result"].as_array().unwrap();
        assert!(!hints.is_empty());
        // Module line gets a routing-target hint.
        assert!(hints
            .iter()
            .any(|h| h["label"].as_str().unwrap().contains("→")));
    }
}
