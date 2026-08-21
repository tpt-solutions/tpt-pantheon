//! Parser unit tests for tpt-telos.
//!
//! Exercises every grammar production in `grammar.ebnf` (modules, attributes,
//! invariants, functions with/without bodies, contracts, `mutate state`,
//! operator precedence, `old(...)`) and a battery of malformed-input cases that
//! must be rejected with an `Err`.

use tpt_telos_parser::ast::*;
use tpt_telos_parser::parse;

fn parse_one(src: &str) -> Module {
    let modules = parse(src).expect("expected successful parse");
    assert_eq!(modules.len(), 1, "expected exactly one module");
    modules.into_iter().next().unwrap()
}

#[test]
fn parses_empty_module() {
    let m = parse_one("module M { }");
    assert_eq!(m.name, "M");
    assert!(m.items.is_empty());
    assert!(m.attributes.is_empty());
}

#[test]
fn parses_multiple_modules() {
    let modules = parse("module A { } module B { }").unwrap();
    assert_eq!(modules.len(), 2);
    assert_eq!(modules[0].name, "A");
    assert_eq!(modules[1].name, "B");
}

#[test]
fn parses_module_attributes() {
    let m = parse_one("@boundary(cpu_bound, zero_allocation) module Ledger { }");
    assert_eq!(m.attributes.len(), 1);
    let a = &m.attributes[0];
    assert_eq!(a.name, "boundary");
    // Two positional flags, no key/value pairs.
    assert_eq!(a.args.len(), 2);
    assert!(matches!(a.args[0], Arg::Flag(ref s) if s == "cpu_bound"));
    assert!(matches!(a.args[1], Arg::Flag(ref s) if s == "zero_allocation"));
}

#[test]
fn parses_attribute_key_value() {
    let m = parse_one("@config(replication_factor = 3) module M { }");
    assert_eq!(m.attributes.len(), 1);
    let a = &m.attributes[0];
    assert_eq!(a.name, "config");
    assert!(matches!(
        &a.args[0],
        Arg::Kv(k, Literal::Int(3)) if k == "replication_factor"
    ));
}

#[test]
fn parses_invariant() {
    let m = parse_one("module M { invariant Wallet { balance >= 0 } }");
    match &m.items[0] {
        Item::Invariant(inv) => {
            assert_eq!(inv.name, "Wallet");
            assert_eq!(inv.constraints.len(), 1);
            assert!(matches!(
                &inv.constraints[0],
                Expr::Bin {
                    op: BinOp::Ge,
                    lhs,
                    rhs,
                } if matches!(lhs.as_ref(), Expr::Var(v) if v == "balance")
                    && matches!(rhs.as_ref(), Expr::Int(0))
            ));
        }
        other => panic!("expected invariant, got {other:?}"),
    }
}

#[test]
fn parses_func_with_full_body() {
    let src = "module M {
        func transfer(from: Wallet, to: Wallet, amount: PositiveInt)
            requires from.balance >= amount
            ensures from.balance == old(from.balance) - amount
        {
            mutate state {
                from.balance -= amount
                to.balance += amount
            }
        }
    }";
    let m = parse_one(src);
    let f = match &m.items[0] {
        Item::Func(f) => f,
        other => panic!("expected func, got {other:?}"),
    };
    assert_eq!(f.name, "transfer");
    assert_eq!(f.params.len(), 3);
    assert_eq!(f.params[0].name, "from");
    assert_eq!(f.params[0].ty.name(), "Wallet");
    assert_eq!(f.requires.len(), 1);
    assert_eq!(f.ensures.len(), 1);
    assert!(!f.elided);
    assert_eq!(f.body.len(), 1);
    match &f.body[0] {
        Stmt::MutateState(assigns) => assert_eq!(assigns.len(), 2),
        other => panic!("expected mutate state, got {other:?}"),
    }
}

#[test]
fn parses_elided_func() {
    let src = "module M {
        func inc(c: Counter)
            requires c.v >= 0
            ensures c.v == old(c.v) + 1
        ;
    }";
    let m = parse_one(src);
    let f = match &m.items[0] {
        Item::Func(f) => f,
        other => panic!("expected func, got {other:?}"),
    };
    assert!(f.elided);
    assert!(f.body.is_empty());
    assert_eq!(f.requires.len(), 1);
    assert_eq!(f.ensures.len(), 1);
}

#[test]
fn parses_func_without_contracts() {
    let src = "module M { func noop(x: i64) { } }";
    let m = parse_one(src);
    let f = match &m.items[0] {
        Item::Func(f) => f,
        other => panic!("expected func, got {other:?}"),
    };
    assert!(f.requires.is_empty());
    assert!(f.ensures.is_empty());
    assert!(f.body.is_empty());
}

#[test]
fn parses_bare_assignment_statement() {
    let src = "module M { func f(x: i64) { x = x + 1; } }";
    let m = parse_one(src);
    let f = match &m.items[0] {
        Item::Func(f) => f,
        other => panic!("expected func, got {other:?}"),
    };
    assert_eq!(f.body.len(), 1);
    match &f.body[0] {
        Stmt::Assign(a) => assert_eq!(a.op, AssignOp::Set),
        other => panic!("expected bare assign, got {other:?}"),
    }
}

#[test]
fn parses_eject_attribute_on_func() {
    let src = "module M { @eject(go) func f(x: T) { } }";
    let m = parse_one(src);
    let f = match &m.items[0] {
        Item::Func(f) => f,
        other => panic!("expected func, got {other:?}"),
    };
    assert!(f.is_ejected());
    assert_eq!(f.eject_lang(), Some("go"));
}

#[test]
fn parses_operator_precedence() {
    // a + b * c == (d - e) / 2
    let src = "module M { func f(x: T) requires a + b * c == (d - e) / 2 { } }";
    let m = parse_one(src);
    let f = match &m.items[0] {
        Item::Func(f) => f,
        other => panic!("expected func, got {other:?}"),
    };
    let e = &f.requires[0];
    // Top level is an equality.
    let (lhs, rhs) = match e {
        Expr::Bin {
            op: BinOp::Eq,
            lhs,
            rhs,
        } => (lhs.as_ref(), rhs.as_ref()),
        other => panic!("expected equality at top, got {other:?}"),
    };
    // Left side is addition.
    assert!(matches!(lhs, Expr::Bin { op: BinOp::Add, .. }));
    // Right side is a division of a parenthesised subtraction.
    match rhs {
        Expr::Bin {
            op: BinOp::Div,
            lhs,
            rhs,
        } => {
            assert!(matches!(lhs.as_ref(), Expr::Bin { op: BinOp::Sub, .. }));
            assert!(matches!(rhs.as_ref(), Expr::Int(2)));
        }
        other => panic!("expected division on right, got {other:?}"),
    }
}

#[test]
fn parses_old_inside_arithmetic() {
    let src = "module M { func f(c: C) ensures c.v == old(c.v) * 2 { } }";
    let m = parse_one(src);
    let f = match &m.items[0] {
        Item::Func(f) => f,
        other => panic!("expected func, got {other:?}"),
    };
    match &f.ensures[0] {
        Expr::Bin {
            op: BinOp::Eq, rhs, ..
        } => {
            // old(c.v) * 2  =>  Mul(Old(Field), Int(2))
            assert!(matches!(
                rhs.as_ref(),
                Expr::Bin {
                    op: BinOp::Mul,
                    lhs,
                    rhs,
                } if matches!(lhs.as_ref(), Expr::Old(_))
                    && matches!(rhs.as_ref(), Expr::Int(2))
            ));
        }
        other => panic!("unexpected ensures: {other:?}"),
    }
}

#[test]
fn parses_logical_and_in_contract() {
    let src = "module M { func f(x: i64, y: i64) requires x >= 0 && y >= 0 { } }";
    let m = parse_one(src);
    let f = match &m.items[0] {
        Item::Func(f) => f,
        other => panic!("expected func, got {other:?}"),
    };
    assert!(matches!(&f.requires[0], Expr::Bin { op: BinOp::And, .. }));
}

// --------------------------------------------------------------------------
// Malformed input.
// --------------------------------------------------------------------------

#[test]
fn rejects_unterminated_module() {
    assert!(parse("module M {").is_err());
}

#[test]
fn rejects_missing_rparen_in_params() {
    assert!(parse("module M { func f(x: T { } }").is_err());
}

#[test]
fn rejects_missing_brace_after_module_name() {
    assert!(parse("module M func f(x: T) { }").is_err());
}

#[test]
fn rejects_unexpected_token_in_expression() {
    assert!(parse("module M { func f(x: T) requires @ { } }").is_err());
}

#[test]
fn rejects_unknown_item_kind() {
    assert!(parse("module M { notanitem }").is_err());
}

#[test]
fn rejects_attribute_on_invariant() {
    assert!(parse("module M { @eject invariant Wallet { balance >= 0 } }").is_err());
}

#[test]
fn rejects_unexpected_token_at_module_level() {
    assert!(parse("module M { } ???").is_err());
}

#[test]
fn rejects_function_without_body_or_semicolon() {
    // A function must either have a `{ ... }` body or be elided with `;`.
    assert!(parse("module M { func f(x: T) requires x >= 0 }").is_err());
}
