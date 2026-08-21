//! Lexer unit tests for tpt-telos.
//!
//! Covers tokenisation of every token class, whitespace/comment handling,
//! integer literals, span tracking, the `int_to_literal` helper, and error
//! reporting for unexpected characters.

use tpt_telos_parser::ast::Literal;
use tpt_telos_parser::lexer::{int_to_literal, lex, Token};

fn toks(src: &str) -> Vec<Token> {
    lex(src).unwrap().into_iter().map(|(t, _, _)| t).collect()
}

#[test]
fn lexes_keywords() {
    let t = toks("module invariant func requires ensures mutate state old");
    assert_eq!(
        t,
        vec![
            Token::KwModule,
            Token::KwInvariant,
            Token::KwFunc,
            Token::KwRequires,
            Token::KwEnsures,
            Token::KwMutate,
            Token::KwState,
            Token::KwOld,
            Token::Eof,
        ]
    );
}

#[test]
fn lexes_idents_and_integers() {
    let t = toks("wallet 42 foo_2 0");
    assert_eq!(
        t,
        vec![
            Token::Ident("wallet".to_string()),
            Token::Int(42),
            Token::Ident("foo_2".to_string()),
            Token::Int(0),
            Token::Eof,
        ]
    );
}

#[test]
fn lexes_single_char_tokens() {
    let t = toks("@(){}:,.;=");
    assert_eq!(
        t,
        vec![
            Token::At,
            Token::LParen,
            Token::RParen,
            Token::LBrace,
            Token::RBrace,
            Token::Colon,
            Token::Comma,
            Token::Dot,
            Token::Semicolon,
            Token::Assign,
            Token::Eof,
        ]
    );
}

#[test]
fn lexes_multi_char_operators() {
    let t = toks("+= -= == != <= >= && || + - * / < >");
    assert_eq!(
        t,
        vec![
            Token::PlusAssign,
            Token::MinusAssign,
            Token::EqEq,
            Token::Ne,
            Token::Le,
            Token::Ge,
            Token::And,
            Token::Or,
            Token::Plus,
            Token::Minus,
            Token::Star,
            Token::Slash,
            Token::Lt,
            Token::Gt,
            Token::Eof,
        ]
    );
}

#[test]
fn skips_whitespace() {
    let t = toks("  \t\n module   \r\n ");
    assert_eq!(t, vec![Token::KwModule, Token::Eof]);
}

#[test]
fn skips_line_comments() {
    // A `//` comment runs to end of line and is discarded.
    let t = toks("module // this is a comment\n wallet");
    assert_eq!(
        t,
        vec![
            Token::KwModule,
            Token::Ident("wallet".to_string()),
            Token::Eof
        ]
    );
}

#[test]
fn skips_comment_with_no_trailing_newline_at_eof() {
    let t = toks("module // trailing comment");
    assert_eq!(t, vec![Token::KwModule, Token::Eof]);
}

#[test]
fn tracks_token_spans() {
    let spanned = lex("ab 12").unwrap();
    // (Ident "ab", 0, 2)
    assert_eq!(spanned[0].0, Token::Ident("ab".to_string()));
    assert_eq!(spanned[0].1, 0);
    assert_eq!(spanned[0].2, 2);
    // (Int 12, 3, 5)
    assert_eq!(spanned[1].0, Token::Int(12));
    assert_eq!(spanned[1].1, 3);
    assert_eq!(spanned[1].2, 5);
    // Eof at end of input.
    let last = spanned.last().unwrap();
    assert_eq!(last.0, Token::Eof);
    assert_eq!(last.1, 5);
    assert_eq!(last.2, 5);
}

#[test]
fn tracks_multibyte_span_offsets() {
    // Operators consume two source characters; offsets must reflect that.
    let spanned = lex("a >= b").unwrap();
    assert_eq!(spanned[0], (Token::Ident("a".to_string()), 0, 1));
    assert_eq!(spanned[1], (Token::Ge, 2, 4));
    assert_eq!(spanned[2], (Token::Ident("b".to_string()), 5, 6));
}

#[test]
fn errors_on_unexpected_character() {
    let err = lex("module # wallet").unwrap_err();
    assert!(
        err.contains('#'),
        "error should mention the bad char: {err}"
    );
    assert!(
        err.contains("offset"),
        "error should report a position: {err}"
    );
}

#[test]
fn int_out_of_range_is_an_error() {
    // i64::MAX + 1 does not fit; the lexer must reject it rather than wrap.
    let too_big = (i64::MAX as u64 + 1).to_string();
    let err = lex(&too_big).unwrap_err();
    assert!(err.contains("out of range"), "error was: {err}");
}

#[test]
fn int_to_literal_helper() {
    assert_eq!(int_to_literal(&Token::Int(7)), Some(Literal::Int(7)));
    assert_eq!(int_to_literal(&Token::Ident("x".into())), None);
}
