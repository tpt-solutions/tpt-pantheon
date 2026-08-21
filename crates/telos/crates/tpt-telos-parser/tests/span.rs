//! Integration tests for source-span tracking on `Func`/`Invariant` AST nodes
//! (Phase 6, deliverable 1: real locations for the LLM diagnostic schema).

use tpt_telos_parser::ast::*;
use tpt_telos_parser::parse;

fn parse_one(src: &str) -> Module {
    let modules = parse(src).expect("expected successful parse");
    assert_eq!(modules.len(), 1, "expected exactly one module");
    modules.into_iter().next().unwrap()
}

#[test]
fn func_span_points_at_func_keyword_single_line() {
    let m = parse_one("module M { func f(x: Int) requires x > 0 ensures x >= 0 ; }");
    let Item::Func(f) = &m.items[0] else {
        panic!("expected a func item");
    };
    assert_eq!(f.span.line, 1);
    // "module M { " is 11 chars, so `func` starts at column 12.
    assert_eq!(f.span.col, 12);
    assert_eq!(f.requires_spans.len(), 1);
    assert_eq!(f.ensures_spans.len(), 1);
    // Both clauses are on the same line as the func declaration here.
    assert_eq!(f.requires_spans[0].line, 1);
    assert_eq!(f.ensures_spans[0].line, 1);
}

#[test]
fn func_and_clause_spans_track_distinct_lines() {
    let src = "module M {\n\
               func f(x: Int)\n\
               requires x > 0\n\
               ensures x >= 0\n\
               ;\n\
               }";
    let m = parse_one(src);
    let Item::Func(f) = &m.items[0] else {
        panic!("expected a func item");
    };
    assert_eq!(f.span.line, 2);
    assert_eq!(f.requires_spans[0].line, 3);
    assert_eq!(f.ensures_spans[0].line, 4);
}

#[test]
fn multiple_ensures_clauses_get_distinct_spans() {
    let src = "module M {\n\
               func f(x: Int)\n\
               ensures x >= 0\n\
               ensures x <= 100\n\
               ;\n\
               }";
    let m = parse_one(src);
    let Item::Func(f) = &m.items[0] else {
        panic!("expected a func item");
    };
    assert_eq!(f.ensures_spans.len(), 2);
    assert_eq!(f.ensures_spans[0].line, 3);
    assert_eq!(f.ensures_spans[1].line, 4);
}

#[test]
fn invariant_span_and_constraint_spans() {
    let src = "module M {\n\
               invariant Wallet {\n\
               balance >= 0\n\
               balance <= 1000\n\
               }\n\
               }";
    let m = parse_one(src);
    let Item::Invariant(inv) = &m.items[0] else {
        panic!("expected an invariant item");
    };
    assert_eq!(inv.span.line, 2);
    assert_eq!(inv.constraint_spans.len(), 2);
    assert_eq!(inv.constraint_spans[0].line, 3);
    assert_eq!(inv.constraint_spans[1].line, 4);
}
