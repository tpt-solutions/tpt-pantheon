//! Tokenizer for tpt-telos.

use crate::ast::Literal;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Ident(String),
    Int(i64),
    At,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LSquare, // [
    RSquare, // ]
    Comma,
    Colon,
    Dot,
    Semicolon,
    Assign,      // =
    PlusAssign,  // +=
    MinusAssign, // -=
    Plus,
    Minus,
    Star,
    Slash,
    EqEq,       // ==
    Ne,         // !=
    Lt,         // <
    Le,         // <=
    Gt,         // >
    Ge,         // >=
    And,        // &&
    Or,         // ||
    Arrow,      // ->
    FatArrow,   // =>
    Question,   // ?
    DotDot,     // ..
    UnderScore, // _
    KwModule,
    KwInvariant,
    KwFunc,
    KwRequires,
    KwEnsures,
    KwMutate,
    KwState,
    KwOld,
    KwLet,
    KwMut,
    KwIf,
    KwElse,
    KwMatch,
    KwStruct,
    KwEnum,
    KwForall,
    KwIn,
    KwPure,
    KwReturn,
    KwResult,
    KwOk,
    KwErr,
    Eof,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Token::Ident(s) => return write!(f, "identifier `{s}`"),
            Token::Int(n) => return write!(f, "integer `{n}`"),
            Token::At => "@",
            Token::LParen => "(",
            Token::RParen => ")",
            Token::LBrace => "{",
            Token::RBrace => "}",
            Token::LSquare => "[",
            Token::RSquare => "]",
            Token::Comma => ",",
            Token::Colon => ":",
            Token::Dot => ".",
            Token::DotDot => "..",
            Token::Semicolon => ";",
            Token::Assign => "=",
            Token::PlusAssign => "+=",
            Token::MinusAssign => "-=",
            Token::Plus => "+",
            Token::Minus => "-",
            Token::Star => "*",
            Token::Slash => "/",
            Token::EqEq => "==",
            Token::Ne => "!=",
            Token::Lt => "<",
            Token::Le => "<=",
            Token::Gt => ">",
            Token::Ge => ">=",
            Token::And => "&&",
            Token::Or => "||",
            Token::Arrow => "->",
            Token::FatArrow => "=>",
            Token::Question => "?",
            Token::UnderScore => "_",
            Token::KwModule => "module",
            Token::KwInvariant => "invariant",
            Token::KwFunc => "func",
            Token::KwRequires => "requires",
            Token::KwEnsures => "ensures",
            Token::KwMutate => "mutate",
            Token::KwState => "state",
            Token::KwOld => "old",
            Token::KwLet => "let",
            Token::KwMut => "mut",
            Token::KwIf => "if",
            Token::KwElse => "else",
            Token::KwMatch => "match",
            Token::KwStruct => "struct",
            Token::KwEnum => "enum",
            Token::KwForall => "forall",
            Token::KwIn => "in",
            Token::KwPure => "pure",
            Token::KwReturn => "return",
            Token::KwResult => "Result",
            Token::KwOk => "Ok",
            Token::KwErr => "Err",
            Token::Eof => "end of file",
        };
        write!(f, "`{s}`")
    }
}

pub type Spanned = (Token, usize, usize);

/// Tokenize a tpt-telos source string into a list of spanned tokens.
///
/// # Examples
///
/// ```
/// use tpt_telos_parser::lexer::lex;
///
/// let tokens = lex("module M {}").unwrap();
/// // The result includes a final Eof sentinel.
/// assert!(!tokens.is_empty());
/// ```
///
/// Returns an error on unrecognised characters:
///
/// ```
/// use tpt_telos_parser::lexer::lex;
///
/// assert!(lex("module M { # }").is_err());
/// ```
pub fn lex(src: &str) -> Result<Vec<Spanned>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    let mut tokens = Vec::new();

    while i < chars.len() {
        let c = chars[i];

        // whitespace
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // line comment
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        let start = i;

        // identifiers / keywords
        if c.is_ascii_alphabetic() || c == '_' {
            let mut s = String::new();
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                s.push(chars[i]);
                i += 1;
            }
            let tok = match s.as_str() {
                "module" => Token::KwModule,
                "invariant" => Token::KwInvariant,
                "func" => Token::KwFunc,
                "requires" => Token::KwRequires,
                "ensures" => Token::KwEnsures,
                "mutate" => Token::KwMutate,
                "state" => Token::KwState,
                "old" => Token::KwOld,
                "let" => Token::KwLet,
                "mut" => Token::KwMut,
                "if" => Token::KwIf,
                "else" => Token::KwElse,
                "match" => Token::KwMatch,
                "struct" => Token::KwStruct,
                "enum" => Token::KwEnum,
                "forall" => Token::KwForall,
                "in" => Token::KwIn,
                "pure" => Token::KwPure,
                "return" => Token::KwReturn,
                "Result" => Token::KwResult,
                "Ok" => Token::KwOk,
                "Err" => Token::KwErr,
                "_" => Token::UnderScore,
                _ => Token::Ident(s),
            };
            tokens.push((tok, start, i));
            continue;
        }

        // integers
        if c.is_ascii_digit() {
            let mut s = String::new();
            while i < chars.len() && chars[i].is_ascii_digit() {
                s.push(chars[i]);
                i += 1;
            }
            let n: i64 = s
                .parse()
                .map_err(|_| format!("line/col: integer literal `{s}` out of range"))?;
            tokens.push((Token::Int(n), start, i));
            continue;
        }

        // multi-character operators
        if c == '+' && i + 1 < chars.len() && chars[i + 1] == '=' {
            tokens.push((Token::PlusAssign, start, i + 2));
            i += 2;
            continue;
        }
        if c == '-' && i + 1 < chars.len() && chars[i + 1] == '=' {
            tokens.push((Token::MinusAssign, start, i + 2));
            i += 2;
            continue;
        }
        if c == '=' && i + 1 < chars.len() && chars[i + 1] == '=' {
            tokens.push((Token::EqEq, start, i + 2));
            i += 2;
            continue;
        }
        if c == '!' && i + 1 < chars.len() && chars[i + 1] == '=' {
            tokens.push((Token::Ne, start, i + 2));
            i += 2;
            continue;
        }
        if c == '<' && i + 1 < chars.len() && chars[i + 1] == '=' {
            tokens.push((Token::Le, start, i + 2));
            i += 2;
            continue;
        }
        if c == '>' && i + 1 < chars.len() && chars[i + 1] == '=' {
            tokens.push((Token::Ge, start, i + 2));
            i += 2;
            continue;
        }
        if c == '&' && i + 1 < chars.len() && chars[i + 1] == '&' {
            tokens.push((Token::And, start, i + 2));
            i += 2;
            continue;
        }
        if c == '|' && i + 1 < chars.len() && chars[i + 1] == '|' {
            tokens.push((Token::Or, start, i + 2));
            i += 2;
            continue;
        }
        if c == '-' && i + 1 < chars.len() && chars[i + 1] == '>' {
            tokens.push((Token::Arrow, start, i + 2));
            i += 2;
            continue;
        }
        if c == '=' && i + 1 < chars.len() && chars[i + 1] == '>' {
            tokens.push((Token::FatArrow, start, i + 2));
            i += 2;
            continue;
        }

        // single-character tokens
        let tok = match c {
            '@' => Token::At,
            '(' => Token::LParen,
            ')' => Token::RParen,
            '{' => Token::LBrace,
            '}' => Token::RBrace,
            '[' => Token::LSquare,
            ']' => Token::RSquare,
            ',' => Token::Comma,
            ':' => Token::Colon,
            '.' => {
                if i + 1 < chars.len() && chars[i + 1] == '.' {
                    i += 1;
                    Token::DotDot
                } else {
                    Token::Dot
                }
            }
            ';' => Token::Semicolon,
            '=' => Token::Assign,
            '+' => Token::Plus,
            '-' => Token::Minus,
            '*' => Token::Star,
            '/' => Token::Slash,
            '<' => Token::Lt,
            '>' => Token::Gt,
            '?' => Token::Question,
            _ => return Err(format!("unexpected character `{c}` at offset {start}")),
        };
        tokens.push((tok, start, i + 1));
        i += 1;
    }

    tokens.push((Token::Eof, chars.len(), chars.len()));
    Ok(tokens)
}

/// Helper used by the parser to turn an INT token into a literal when needed.
pub fn int_to_literal(tok: &Token) -> Option<Literal> {
    match tok {
        Token::Int(n) => Some(Literal::Int(*n)),
        _ => None,
    }
}
