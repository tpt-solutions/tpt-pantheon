//! Recursive-descent parser for tpt-telos, building the AST.

use crate::ast::*;
use crate::lexer::{lex, Token};
use crate::span::{LineIndex, Span};

pub struct Parser {
    tokens: Vec<(Token, usize, usize)>,
    pos: usize,
    line_index: LineIndex,
}

impl Parser {
    fn new(src: &str, tokens: Vec<(Token, usize, usize)>) -> Self {
        Parser {
            tokens,
            pos: 0,
            line_index: LineIndex::new(src),
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos].0
    }

    fn advance(&mut self) -> Token {
        let (tok, _, _) = self.tokens[self.pos].clone();
        self.pos += 1;
        tok
    }

    /// Source span of the token about to be consumed (i.e. `self.peek()`).
    fn current_span(&self) -> Span {
        let start_offset = self.tokens[self.pos].1;
        self.line_index.span_at(start_offset)
    }

    fn expect(&mut self, expected: Token) -> Result<(), String> {
        let got = self.peek().clone();
        if got == expected {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected {}, found {}", expected, got))
        }
    }

    fn expect_ident(&mut self) -> Result<String, String> {
        match self.peek().clone() {
            Token::Ident(s) => {
                self.pos += 1;
                Ok(s)
            }
            other => Err(format!("expected identifier, found {}", other)),
        }
    }

    /// Accept an identifier or a keyword (used for attribute names like
    /// `@state(...)`, where `state` is a keyword).
    fn expect_ident_or_keyword(&mut self) -> Result<String, String> {
        match self.peek().clone() {
            Token::Ident(s) => {
                self.pos += 1;
                Ok(s)
            }
            Token::KwState => {
                self.pos += 1;
                Ok("state".to_string())
            }
            other => Err(format!("expected identifier, found {}", other)),
        }
    }

    #[allow(dead_code)]
    fn at_stmt_start(&self) -> bool {
        matches!(
            self.peek(),
            Token::KwMutate
                | Token::KwLet
                | Token::KwIf
                | Token::KwMatch
                | Token::KwReturn
                | Token::Ident(_)
                | Token::KwOld
                | Token::LParen
                | Token::KwResult
                | Token::KwOk
                | Token::KwErr
        )
    }

    fn at_expr_start(&self) -> bool {
        matches!(
            self.peek(),
            Token::Int(_)
                | Token::Ident(_)
                | Token::KwOld
                | Token::LParen
                | Token::Minus
                | Token::KwIf
                | Token::KwMatch
                | Token::KwForall
                | Token::KwResult
                | Token::KwOk
                | Token::KwErr
        )
    }

    // ---- program ----

    /// Parse a full source string into a list of modules.
    pub fn parse_source(src: &str) -> Result<Vec<Module>, String> {
        let tokens = lex(src)?;
        let mut p = Parser::new(src, tokens);
        let mut modules = Vec::new();
        while *p.peek() != Token::Eof {
            modules.push(p.parse_module()?);
        }
        Ok(modules)
    }

    fn parse_module(&mut self) -> Result<Module, String> {
        let mut attributes = Vec::new();
        while *self.peek() == Token::At {
            attributes.push(self.parse_attribute()?);
        }
        self.expect(Token::KwModule)?;
        let name = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        let mut items = Vec::new();
        while *self.peek() != Token::RBrace {
            items.push(self.parse_item()?);
        }
        self.expect(Token::RBrace)?;
        Ok(Module {
            attributes,
            name,
            items,
        })
    }

    fn parse_attribute(&mut self) -> Result<Attribute, String> {
        self.expect(Token::At)?;
        let name = self.expect_ident_or_keyword()?;
        let mut args = Vec::new();
        if *self.peek() == Token::LParen {
            self.advance();
            if *self.peek() != Token::RParen {
                loop {
                    let key = self.expect_ident()?;
                    if *self.peek() == Token::Assign {
                        self.advance();
                        let lit = match self.peek().clone() {
                            Token::Int(n) => {
                                self.advance();
                                Literal::Int(n)
                            }
                            Token::Ident(s) => {
                                self.advance();
                                Literal::Ident(s)
                            }
                            other => {
                                return Err(format!(
                                    "expected literal in attribute, found {}",
                                    other
                                ))
                            }
                        };
                        args.push(Arg::Kv(key, lit));
                    } else {
                        args.push(Arg::Flag(key));
                    }
                    if *self.peek() == Token::Comma {
                        self.advance();
                        continue;
                    }
                    break;
                }
            }
            self.expect(Token::RParen)?;
        }
        Ok(Attribute { name, args })
    }

    // ---- items ----

    fn parse_item(&mut self) -> Result<Item, String> {
        let mut attributes = Vec::new();
        while *self.peek() == Token::At {
            attributes.push(self.parse_attribute()?);
        }
        match self.peek() {
            Token::KwInvariant => {
                if !attributes.is_empty() {
                    return Err("attributes are not supported on `invariant` items".to_string());
                }
                Ok(Item::Invariant(self.parse_invariant()?))
            }
            Token::KwFunc => Ok(Item::Func(self.parse_func(attributes)?)),
            Token::KwStruct => {
                if !attributes.is_empty() {
                    return Err("attributes are not supported on `struct` items".to_string());
                }
                Ok(Item::Struct(self.parse_struct_def()?))
            }
            Token::KwEnum => {
                if !attributes.is_empty() {
                    return Err("attributes are not supported on `enum` items".to_string());
                }
                Ok(Item::Enum(self.parse_enum_def()?))
            }
            other => Err(format!(
                "expected `invariant`, `func`, `struct`, or `enum`, found {}",
                other
            )),
        }
    }

    fn parse_invariant(&mut self) -> Result<Invariant, String> {
        let span = self.current_span();
        self.expect(Token::KwInvariant)?;
        let name = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        let mut constraints = Vec::new();
        let mut constraint_spans = Vec::new();
        while *self.peek() != Token::RBrace {
            constraint_spans.push(self.current_span());
            constraints.push(self.parse_expr()?);
            if *self.peek() == Token::Semicolon {
                self.advance();
            }
        }
        self.expect(Token::RBrace)?;
        Ok(Invariant {
            name,
            constraints,
            span,
            constraint_spans,
        })
    }

    fn parse_struct_def(&mut self) -> Result<StructDef, String> {
        self.expect(Token::KwStruct)?;
        let name = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        let mut fields = Vec::new();
        while *self.peek() != Token::RBrace {
            let field_name = self.expect_ident()?;
            self.expect(Token::Colon)?;
            let ty = self.parse_type()?;
            fields.push(FieldDef {
                name: field_name,
                ty,
            });
            if *self.peek() == Token::Comma {
                self.advance();
            }
        }
        self.expect(Token::RBrace)?;
        Ok(StructDef { name, fields })
    }

    fn parse_enum_def(&mut self) -> Result<EnumDef, String> {
        self.expect(Token::KwEnum)?;
        let name = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        let mut variants = Vec::new();
        while *self.peek() != Token::RBrace {
            let variant_name = self.expect_ident()?;
            let fields = if *self.peek() == Token::LBrace {
                self.advance();
                let mut fields = Vec::new();
                while *self.peek() != Token::RBrace {
                    let field_name = self.expect_ident()?;
                    self.expect(Token::Colon)?;
                    let ty = self.parse_type()?;
                    fields.push(FieldDef {
                        name: field_name,
                        ty,
                    });
                    if *self.peek() == Token::Comma {
                        self.advance();
                    }
                }
                self.expect(Token::RBrace)?;
                fields
            } else {
                Vec::new()
            };
            variants.push(VariantDef {
                name: variant_name,
                fields,
            });
            if *self.peek() == Token::Comma {
                self.advance();
            }
        }
        self.expect(Token::RBrace)?;
        Ok(EnumDef { name, variants })
    }

    // ---- functions ----

    fn parse_func(&mut self, attributes: Vec<Attribute>) -> Result<Func, String> {
        let span = self.current_span();
        self.expect(Token::KwFunc)?;
        let name = self.expect_ident()?;
        self.expect(Token::LParen)?;
        let mut params = Vec::new();
        if *self.peek() != Token::RParen {
            loop {
                let mutability = if *self.peek() == Token::KwMut {
                    self.advance();
                    ParamMutability::Mutable
                } else {
                    ParamMutability::Immutable
                };
                let pname = self.expect_ident()?;
                self.expect(Token::Colon)?;
                let ty = self.parse_type()?;
                params.push(Param {
                    name: pname,
                    ty,
                    mutability,
                });
                if *self.peek() == Token::Comma {
                    self.advance();
                    continue;
                }
                break;
            }
        }
        self.expect(Token::RParen)?;

        // Optional return type: `-> Type`
        let return_ty = if *self.peek() == Token::Arrow {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };

        let mut requires = Vec::new();
        let mut ensures = Vec::new();
        let mut requires_spans = Vec::new();
        let mut ensures_spans = Vec::new();
        while *self.peek() == Token::KwRequires || *self.peek() == Token::KwEnsures {
            if *self.peek() == Token::KwRequires {
                self.advance();
                requires_spans.push(self.current_span());
                requires.push(self.parse_expr()?);
            } else {
                self.advance();
                ensures_spans.push(self.current_span());
                ensures.push(self.parse_expr()?);
            }
        }

        // Body may be elided with `;` (intent-only) or given as `{ ... }`.
        let (body, elided) = if *self.peek() == Token::Semicolon {
            self.advance();
            (Vec::new(), true)
        } else {
            self.expect(Token::LBrace)?;
            let mut b = Vec::new();
            while *self.peek() != Token::RBrace {
                b.push(self.parse_stmt()?);
            }
            self.expect(Token::RBrace)?;
            (b, false)
        };

        Ok(Func {
            attributes,
            name,
            params,
            return_ty,
            requires,
            ensures,
            body,
            elided,
            span,
            requires_spans,
            ensures_spans,
        })
    }

    // ---- types ----

    fn parse_type(&mut self) -> Result<Type, String> {
        match self.peek().clone() {
            Token::KwResult => {
                self.advance();
                self.expect(Token::Lt)?;
                let ok = self.parse_type()?;
                self.expect(Token::Comma)?;
                let err = self.parse_type()?;
                self.expect(Token::Gt)?;
                Ok(Type::result(ok, err))
            }
            Token::LParen => {
                self.advance();
                let mut elems = Vec::new();
                elems.push(self.parse_type()?);
                while *self.peek() == Token::Comma {
                    self.advance();
                    elems.push(self.parse_type()?);
                }
                self.expect(Token::RParen)?;
                if elems.len() == 1 {
                    Ok(elems.remove(0))
                } else {
                    Ok(Type::Tuple(elems))
                }
            }
            Token::LSquare => {
                // Array or slice type: `[T; N]` or `[T]`
                self.advance();
                let elem_ty = self.parse_type()?;
                if *self.peek() == Token::Semicolon {
                    self.advance();
                    let len = match self.peek().clone() {
                        Token::Int(n) => {
                            self.advance();
                            n as usize
                        }
                        other => return Err(format!("expected array length, found {}", other)),
                    };
                    self.expect(Token::RSquare)?;
                    Ok(Type::Array(Box::new(elem_ty), len))
                } else {
                    self.expect(Token::RSquare)?;
                    Ok(Type::Slice(Box::new(elem_ty)))
                }
            }
            Token::Ident(name) => {
                self.advance();
                // Check for generic type: `Name<T, U>`
                if *self.peek() == Token::Lt {
                    self.advance();
                    let mut args = Vec::new();
                    args.push(self.parse_type()?);
                    while *self.peek() == Token::Comma {
                        self.advance();
                        args.push(self.parse_type()?);
                    }
                    self.expect(Token::Gt)?;
                    Ok(Type::Generic(name, args))
                } else {
                    Ok(Type::Named(name))
                }
            }
            other => Err(format!("expected type, found {}", other)),
        }
    }

    // ---- statements ----

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        match self.peek().clone() {
            Token::KwMutate => {
                self.advance();
                self.expect(Token::KwState)?;
                self.expect(Token::LBrace)?;
                let mut assigns = Vec::new();
                while *self.peek() != Token::RBrace {
                    assigns.push(self.parse_assign()?);
                }
                self.expect(Token::RBrace)?;
                Ok(Stmt::MutateState(assigns))
            }
            Token::KwLet => {
                self.advance();
                let name = self.expect_ident()?;
                let ty = if *self.peek() == Token::Colon {
                    self.advance();
                    Some(self.parse_type()?)
                } else {
                    None
                };
                self.expect(Token::Assign)?;
                let value = self.parse_expr()?;
                if *self.peek() == Token::Semicolon {
                    self.advance();
                }
                Ok(Stmt::Let(LetBinding { name, ty, value }))
            }
            Token::KwIf => self.parse_if_stmt_as_stmt(),
            Token::KwMatch => self.parse_match_stmt(),
            Token::KwReturn => {
                self.advance();
                if *self.peek() == Token::Semicolon {
                    self.advance();
                    Ok(Stmt::Return(None))
                } else if self.at_expr_start() {
                    let expr = self.parse_expr()?;
                    if *self.peek() == Token::Semicolon {
                        self.advance();
                    }
                    Ok(Stmt::Return(Some(expr)))
                } else {
                    if *self.peek() == Token::Semicolon {
                        self.advance();
                    }
                    Ok(Stmt::Return(None))
                }
            }
            _ => {
                // Could be an assignment or expression statement.
                // We need to check if this is an assignment (has =, +=, -= after expr).
                let expr = self.parse_expr()?;
                match self.peek() {
                    Token::Assign | Token::PlusAssign | Token::MinusAssign => {
                        let op = match self.advance() {
                            Token::Assign => AssignOp::Set,
                            Token::PlusAssign => AssignOp::Add,
                            Token::MinusAssign => AssignOp::Sub,
                            _ => unreachable!(),
                        };
                        let value = self.parse_expr()?;
                        if *self.peek() == Token::Semicolon {
                            self.advance();
                        }
                        Ok(Stmt::Assign(Assign {
                            target: expr,
                            op,
                            value,
                        }))
                    }
                    _ => {
                        // Expression statement (e.g., function call)
                        if *self.peek() == Token::Semicolon {
                            self.advance();
                        }
                        Ok(Stmt::Assign(Assign {
                            target: expr,
                            op: AssignOp::Set,
                            value: Expr::Int(0), // placeholder
                        }))
                    }
                }
            }
        }
    }

    fn parse_if_stmt_as_stmt(&mut self) -> Result<Stmt, String> {
        self.expect(Token::KwIf)?;
        let condition = self.parse_expr()?;
        self.expect(Token::LBrace)?;
        let mut then_body = Vec::new();
        while *self.peek() != Token::RBrace {
            then_body.push(self.parse_stmt()?);
        }
        self.expect(Token::RBrace)?;
        let else_body = if *self.peek() == Token::KwElse {
            self.advance();
            self.expect(Token::LBrace)?;
            let mut body = Vec::new();
            while *self.peek() != Token::RBrace {
                body.push(self.parse_stmt()?);
            }
            self.expect(Token::RBrace)?;
            Some(body)
        } else {
            None
        };
        Ok(Stmt::If(IfStmt {
            condition,
            then_body,
            else_body,
        }))
    }

    fn parse_match_stmt(&mut self) -> Result<Stmt, String> {
        self.expect(Token::KwMatch)?;
        let scrutinee = self.parse_expr()?;
        self.expect(Token::LBrace)?;
        let mut arms = Vec::new();
        while *self.peek() != Token::RBrace {
            let pattern = self.parse_pattern()?;
            self.expect(Token::FatArrow)?;
            if *self.peek() == Token::LBrace {
                self.advance();
                let mut body = Vec::new();
                while *self.peek() != Token::RBrace {
                    body.push(self.parse_stmt()?);
                }
                self.expect(Token::RBrace)?;
                arms.push(MatchArm { pattern, body });
            } else {
                let stmt = self.parse_stmt()?;
                arms.push(MatchArm {
                    pattern,
                    body: vec![stmt],
                });
            }
        }
        self.expect(Token::RBrace)?;
        Ok(Stmt::Match(MatchStmt { scrutinee, arms }))
    }

    fn parse_assign(&mut self) -> Result<Assign, String> {
        let target = self.parse_expr()?;
        let op = match self.peek().clone() {
            Token::Assign => AssignOp::Set,
            Token::PlusAssign => AssignOp::Add,
            Token::MinusAssign => AssignOp::Sub,
            other => return Err(format!("expected assignment operator, found {}", other)),
        };
        self.advance();
        let value = self.parse_expr()?;
        if *self.peek() == Token::Semicolon {
            self.advance();
        }
        Ok(Assign { target, op, value })
    }

    // ---- patterns ----

    fn parse_pattern(&mut self) -> Result<Pattern, String> {
        match self.peek().clone() {
            Token::Int(n) => {
                self.advance();
                Ok(Pattern::Literal(n))
            }
            Token::Minus => {
                self.advance();
                if let Token::Int(n) = self.peek().clone() {
                    self.advance();
                    Ok(Pattern::Literal(-n))
                } else {
                    Err("expected integer after `-` in pattern".to_string())
                }
            }
            Token::Ident(name) => {
                self.advance();
                if *self.peek() == Token::LParen {
                    self.advance();
                    let mut fields = Vec::new();
                    if *self.peek() != Token::RParen {
                        fields.push(self.parse_pattern()?);
                        while *self.peek() == Token::Comma {
                            self.advance();
                            fields.push(self.parse_pattern()?);
                        }
                    }
                    self.expect(Token::RParen)?;
                    Ok(Pattern::Constructor(name, fields))
                } else {
                    Ok(Pattern::Var(name))
                }
            }
            Token::UnderScore => {
                self.advance();
                Ok(Pattern::Wildcard)
            }
            other => Err(format!("expected pattern, found {}", other)),
        }
    }

    // ---- expressions ----

    fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_and()?;
        while *self.peek() == Token::Or {
            self.advance();
            let rhs = self.parse_and()?;
            lhs = Expr::Bin {
                op: BinOp::Or,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_cmp()?;
        while *self.peek() == Token::And {
            self.advance();
            let rhs = self.parse_cmp()?;
            lhs = Expr::Bin {
                op: BinOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_cmp(&mut self) -> Result<Expr, String> {
        let lhs = self.parse_add()?;
        let op = match self.peek() {
            Token::EqEq => BinOp::Eq,
            Token::Ne => BinOp::Ne,
            Token::Lt => BinOp::Lt,
            Token::Le => BinOp::Le,
            Token::Gt => BinOp::Gt,
            Token::Ge => BinOp::Ge,
            _ => return Ok(lhs),
        };
        self.advance();
        let rhs = self.parse_add()?;
        Ok(Expr::Bin {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
    }

    fn parse_add(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_mul()?;
            lhs = Expr::Bin {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_unary()?;
            lhs = Expr::Bin {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        if *self.peek() == Token::Minus {
            self.advance();
            let expr = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnOp::Neg,
                expr: Box::new(expr),
            });
        }
        self.parse_postfix()
    }

    /// Parse postfix operators: function calls, method calls, index, `?`.
    fn parse_postfix(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek().clone() {
                Token::LParen => {
                    // Function call
                    self.advance();
                    let mut args = Vec::new();
                    if *self.peek() != Token::RParen {
                        args.push(self.parse_expr()?);
                        while *self.peek() == Token::Comma {
                            self.advance();
                            args.push(self.parse_expr()?);
                        }
                    }
                    self.expect(Token::RParen)?;
                    match expr {
                        Expr::Var(name) => {
                            expr = Expr::Call(CallExpr { func: name, args });
                        }
                        Expr::Field { base, field } => {
                            expr = Expr::MethodCall(MethodCallExpr {
                                receiver: Box::new(Expr::Var(base)),
                                method: field,
                                args,
                            });
                        }
                        Expr::MethodCall(m) => {
                            // Chain: a.b(args1)(args2) -- treat as method call on result
                            // For simplicity, nest as a call on the method call result
                            expr = Expr::Call(CallExpr {
                                func: format!("{}.{}", pretty_simple(&m.receiver), m.method),
                                args,
                            });
                        }
                        _ => {
                            return Err(format!(
                                "cannot call non-function expression `{}`",
                                pretty_simple(&expr)
                            ));
                        }
                    }
                }
                Token::Dot => {
                    self.advance();
                    let field = self.expect_ident()?;
                    expr = match expr {
                        Expr::Var(name) => Expr::Field { base: name, field },
                        other => {
                            // Method-style: receiver.field
                            // Could be a method call if followed by `(`
                            Expr::MethodCall(MethodCallExpr {
                                receiver: Box::new(other),
                                method: field,
                                args: Vec::new(),
                            })
                        }
                    };
                }
                Token::LSquare => {
                    // Index: expr[index]
                    self.advance();
                    let index = self.parse_expr()?;
                    self.expect(Token::RSquare)?;
                    expr = Expr::Index(IndexExpr {
                        receiver: Box::new(expr),
                        index: Box::new(index),
                    });
                }
                Token::Question => {
                    self.advance();
                    expr = Expr::Try(Box::new(expr));
                }
                Token::DotDot => {
                    self.advance();
                    let hi = self.parse_expr()?;
                    expr = Expr::Range {
                        lo: Box::new(expr),
                        hi: Box::new(hi),
                    };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.peek().clone() {
            Token::Int(n) => {
                self.advance();
                Ok(Expr::Int(n))
            }
            Token::KwOld => {
                self.advance();
                self.expect(Token::LParen)?;
                let e = self.parse_expr()?;
                self.expect(Token::RParen)?;
                Ok(Expr::Old(Box::new(e)))
            }
            Token::LParen => {
                self.advance();
                let e = self.parse_expr()?;
                self.expect(Token::RParen)?;
                Ok(e)
            }
            Token::KwIf => self.parse_if_expr(),
            Token::KwMatch => self.parse_match_expr(),
            Token::KwForall => self.parse_forall_expr(),
            Token::KwResult => {
                self.advance();
                // Result type used as constructor: Result::Ok / Result::Err
                // But could also be generic type in expression context
                // For now, treat as a variable name
                Ok(Expr::Var("Result".to_string()))
            }
            Token::KwOk => {
                self.advance();
                self.expect(Token::LParen)?;
                let e = self.parse_expr()?;
                self.expect(Token::RParen)?;
                // Represent Ok(value) as a call expression
                Ok(Expr::Call(CallExpr {
                    func: "Ok".to_string(),
                    args: vec![e],
                }))
            }
            Token::KwErr => {
                self.advance();
                self.expect(Token::LParen)?;
                let e = self.parse_expr()?;
                self.expect(Token::RParen)?;
                Ok(Expr::Call(CallExpr {
                    func: "Err".to_string(),
                    args: vec![e],
                }))
            }
            Token::Ident(s) => {
                self.advance();
                // Check for aggregate functions: sum, min, max, count
                // Only treat as aggregate if followed by `(` to avoid
                // conflicting with variable names like `count`.
                match s.as_str() {
                    "sum" | "min" | "max" | "count" if *self.peek() == Token::LParen => {
                        self.expect(Token::LParen)?;
                        let mut args = Vec::new();
                        if *self.peek() != Token::RParen {
                            args.push(self.parse_expr()?);
                            while *self.peek() == Token::Comma {
                                self.advance();
                                args.push(self.parse_expr()?);
                            }
                        }
                        self.expect(Token::RParen)?;
                        let op = match s.as_str() {
                            "sum" => AggregateOp::Sum,
                            "min" => AggregateOp::Min,
                            "max" => AggregateOp::Max,
                            "count" => AggregateOp::Count,
                            _ => unreachable!(),
                        };
                        Ok(Expr::Aggregate(AggregateExpr { op, args }))
                    }
                    _ => {
                        if *self.peek() == Token::Dot {
                            self.advance();
                            let field = self.expect_ident()?;
                            Ok(Expr::Field { base: s, field })
                        } else {
                            Ok(Expr::Var(s))
                        }
                    }
                }
            }
            other => Err(format!("expected expression, found {}", other)),
        }
    }

    fn parse_if_expr(&mut self) -> Result<Expr, String> {
        self.expect(Token::KwIf)?;
        let condition = Box::new(self.parse_expr()?);
        self.expect(Token::LBrace)?;
        let then_expr = Box::new(self.parse_expr()?);
        self.expect(Token::RBrace)?;
        self.expect(Token::KwElse)?;
        self.expect(Token::LBrace)?;
        let else_expr = Box::new(self.parse_expr()?);
        self.expect(Token::RBrace)?;
        Ok(Expr::If(IfExpr {
            condition,
            then_expr,
            else_expr,
        }))
    }

    fn parse_match_expr(&mut self) -> Result<Expr, String> {
        self.expect(Token::KwMatch)?;
        let scrutinee = Box::new(self.parse_expr()?);
        self.expect(Token::LBrace)?;
        let mut arms = Vec::new();
        while *self.peek() != Token::RBrace {
            let pattern = self.parse_pattern()?;
            self.expect(Token::FatArrow)?;
            let expr = self.parse_expr()?;
            arms.push(ExprMatchArm { pattern, expr });
            if *self.peek() == Token::Comma {
                self.advance();
            }
        }
        self.expect(Token::RBrace)?;
        Ok(Expr::Match(MatchExpr { scrutinee, arms }))
    }

    fn parse_forall_expr(&mut self) -> Result<Expr, String> {
        self.expect(Token::KwForall)?;
        let var = self.expect_ident()?;
        self.expect(Token::Colon)?;
        let var_ty = self.parse_type()?;
        let domain = if *self.peek() == Token::KwIn {
            self.advance();
            Some(Box::new(self.parse_expr()?))
        } else {
            None
        };
        self.expect(Token::LBrace)?;
        let body = Box::new(self.parse_expr()?);
        self.expect(Token::RBrace)?;
        Ok(Expr::Forall(ForallExpr {
            var,
            var_ty,
            domain,
            body,
        }))
    }
}

/// Minimal pretty-printer for expressions (used in error messages).
fn pretty_simple(e: &Expr) -> String {
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Var(v) => v.clone(),
        Expr::Field { base, field } => format!("{}.{}", base, field),
        Expr::Old(inner) => format!("old({})", pretty_simple(inner)),
        Expr::Unary { .. } => "<expr>".to_string(),
        Expr::Bin { .. } => "<expr>".to_string(),
        Expr::Call(c) => format!("{}(...)", c.func),
        Expr::MethodCall(m) => format!("{}.{}(...)", pretty_simple(&m.receiver), m.method),
        Expr::Index(i) => format!("{}[...]", pretty_simple(&i.receiver)),
        Expr::If(_) => "<if-expr>".to_string(),
        Expr::Match(_) => "<match-expr>".to_string(),
        Expr::Try(inner) => format!("{}?", pretty_simple(inner)),
        Expr::Forall(_) => "<forall>".to_string(),
        Expr::Aggregate(a) => format!("{}(...)", a.op.op_name()),
        Expr::Range { .. } => "<range>".to_string(),
    }
}

/// Convenience: parse a full source string into a list of modules.
///
/// # Examples
///
/// ```
/// use tpt_telos_parser::parse;
///
/// let src = r#"
///     module Bank {
///         invariant Wallet { balance >= 0 }
///
///         func deposit(w: Wallet, amount: PositiveInt)
///             requires amount > 0
///             ensures w.balance == old(w.balance) + amount
///         ;
///     }
/// "#;
///
/// let modules = parse(src).unwrap();
/// assert_eq!(modules.len(), 1);
/// assert_eq!(modules[0].name, "Bank");
/// assert_eq!(modules[0].items.len(), 2); // Wallet invariant + deposit func
/// ```
///
/// Parse errors return an `Err`:
///
/// ```
/// use tpt_telos_parser::parse;
///
/// assert!(parse("module {").is_err());
/// ```
pub fn parse(src: &str) -> Result<Vec<Module>, String> {
    Parser::parse_source(src)
}
