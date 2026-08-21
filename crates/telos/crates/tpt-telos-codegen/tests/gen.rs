use tpt_telos_agent::{transpile_module, StaticAgent};
use tpt_telos_codegen::{generate_program, generate_project};
use tpt_telos_parser::parse;

fn outcomes_for(
    src: &str,
) -> (
    Vec<tpt_telos_parser::ast::Module>,
    Vec<tpt_telos_agent::FuncOutcome>,
) {
    let modules = parse(src).unwrap();
    let agent = StaticAgent::new();
    let mut outcomes = Vec::new();
    for m in &modules {
        outcomes.extend(transpile_module(m, &agent).unwrap());
    }
    (modules, outcomes)
}

#[test]
fn generated_rust_compiles_for_wallet() {
    let src = std::fs::read_to_string("../../examples/wallet.telos").unwrap();
    let modules = parse(&src).unwrap();
    let agent = StaticAgent::new();
    let mut outcomes = Vec::new();
    for m in &modules {
        outcomes.extend(transpile_module(m, &agent).unwrap());
    }
    let rust = generate_program(&modules, &outcomes);

    assert!(rust.contains("pub struct Wallet"));
    assert!(rust.contains("pub fn transfer(from: &mut Wallet, to: &mut Wallet, amount: i64)"));
    assert!(rust.contains("from.balance -= amount"));
    assert!(rust.contains("to.balance += amount"));
    // Contracts preserved faithfully in doc-comments.
    assert!(rust.contains("ensures:  from.balance == old(from.balance) - amount"));
}

#[test]
fn elided_func_synthesizes_body() {
    let src = std::fs::read_to_string("../../examples/intent.telos").unwrap();
    let modules = parse(&src).unwrap();
    let agent = StaticAgent::new();
    let mut outcomes = Vec::new();
    for m in &modules {
        outcomes.extend(transpile_module(m, &agent).unwrap());
    }
    let rust = generate_program(&modules, &outcomes);
    assert!(rust.contains("c.value = c.value + by"));
    assert!(rust.contains("c.value = c.value - by"));
}

#[test]
fn microservice_routes_modules_to_both_backends() {
    let src = std::fs::read_to_string("../../examples/microservice.telos").unwrap();
    let (modules, outcomes) = outcomes_for(&src);
    let project = generate_project(&modules, &outcomes).unwrap();

    assert!(project.has_rust);
    assert!(project.has_go);
    assert!(project.has_ffi);

    let file = |p: &str| {
        project
            .files
            .iter()
            .find(|f| f.path == p)
            .unwrap_or_else(|| panic!("missing file {p}"))
            .contents
            .clone()
    };

    // Rust backend gets the CPU-bound Ledger module only.
    let lib = file("rust/src/lib.rs");
    assert!(lib.contains("pub struct Account"));
    assert!(lib.contains("pub fn settle(acct: &mut Account, amount: i64)"));
    assert!(lib.contains("pub mod ffi;"));
    assert!(!lib.contains("Queue")); // Go module must not leak into Rust.

    // Go backend gets the network-facing GatewayApi module only.
    let svc = file("go/service.go");
    assert!(svc.contains("package gosvc"));
    assert!(svc.contains("type Queue struct"));
    assert!(svc.contains("Pending int64"));
    assert!(svc.contains("func Enqueue(q *Queue, count int64)"));
    assert!(svc.contains("q.Pending += count"));
    assert!(!svc.contains("Account")); // Rust module must not leak into Go.
}

#[test]
fn ffi_bridge_is_bidirectional() {
    let src = std::fs::read_to_string("../../examples/microservice.telos").unwrap();
    let (modules, outcomes) = outcomes_for(&src);
    let project = generate_project(&modules, &outcomes).unwrap();
    let file = |p: &str| {
        project
            .files
            .iter()
            .find(|f| f.path == p)
            .unwrap()
            .contents
            .clone()
    };

    // C header exposes the Rust functions for Go to call.
    let header = file("go/telos_ffi.h");
    assert!(header.contains("void telos_Ledger_settle(int64_t* acct_balance, int64_t amount);"));

    // Rust side: exports Rust fns + imports Go fns with safe wrappers.
    let rust_ffi = file("rust/src/ffi.rs");
    assert!(rust_ffi
        .contains("pub extern \"C\" fn telos_Ledger_settle(acct_balance: *mut i64, amount: i64)"));
    assert!(rust_ffi.contains("fn telos_GatewayApi_enqueue(q_pending: *mut i64, count: i64);"));
    assert!(rust_ffi.contains("pub fn call_go_enqueue(q_pending: &mut i64, count: i64)"));

    // Go side: cgo calls into Rust + //export shims exposing Go to Rust.
    let go_ffi = file("go/ffi.go");
    assert!(go_ffi.contains("func CallRustSettle(acct *Account, amount int64)"));
    assert!(go_ffi.contains("C.telos_Ledger_settle(&c_acct_balance, C.int64_t(amount))"));
    assert!(go_ffi.contains("//export telos_GatewayApi_enqueue"));
    assert!(go_ffi.contains("Enqueue(&q, int64(count))"));
}
