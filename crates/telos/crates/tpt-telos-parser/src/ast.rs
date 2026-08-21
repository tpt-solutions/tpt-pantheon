//! Abstract Syntax Tree for tpt-telos.
//!
//! The grammar is intentionally "semantically erased": no implicit coercion,
//! every operation is named explicitly. See `grammar.ebnf` for the formal
//! specification.

use crate::span::Span;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// A user or built-in named type, e.g. `Wallet`, `PositiveInt`.
    Named(String),
    /// A generic type, e.g. `Result<Int, String>`, `Vec<Int>`.
    Generic(String, Vec<Type>),
    /// A tuple type, e.g. `(Int, String)`.
    Tuple(Vec<Type>),
    /// A fixed-length array type, e.g. `[Int; 5]`.
    Array(Box<Type>, usize),
    /// A slice type, e.g. `[Int]`.
    Slice(Box<Type>),
}

impl Type {
    /// Return the name of this type (the outermost constructor name).
    ///
    /// # Examples
    ///
    /// ```
    /// use tpt_telos_parser::ast::Type;
    ///
    /// let ty = Type::Named("Wallet".to_string());
    /// assert_eq!(ty.name(), "Wallet");
    /// ```
    pub fn name(&self) -> &str {
        match self {
            Type::Named(s) => s,
            Type::Generic(s, _) => s,
            Type::Tuple(_) => "Tuple",
            Type::Array(_, _) => "Array",
            Type::Slice(_) => "Slice",
        }
    }

    /// Convenience: build `Result<T, E>`.
    pub fn result(ok: Type, err: Type) -> Self {
        Type::Generic("Result".into(), vec![ok, err])
    }

    /// Convenience: build `Int`.
    pub fn int() -> Self {
        Type::Named("Int".into())
    }

    /// Convenience: build `PositiveInt`.
    pub fn positive_int() -> Self {
        Type::Named("PositiveInt".into())
    }

    /// Convenience: build `Bool`.
    pub fn bool() -> Self {
        Type::Named("Bool".into())
    }

    /// Convenience: build `String` (the Telos string type, not Rust's).
    pub fn string() -> Self {
        Type::Named("String".into())
    }
}

// ---------------------------------------------------------------------------
// Literals & attributes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Literal {
    Int(i64),
    Ident(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    /// `@boundary(cpu_bound)`
    Flag(String),
    /// `@state(replication_factor = 3)`
    Kv(String, Literal),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub name: String,
    pub args: Vec<Arg>,
}

// ---------------------------------------------------------------------------
// Modules & items
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub attributes: Vec<Attribute>,
    pub name: String,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Invariant(Invariant),
    Func(Func),
    Struct(StructDef),
    Enum(EnumDef),
}

impl Item {
    /// Return the name of this item (function name, invariant type name,
    /// struct name, or enum name).
    ///
    /// # Examples
    ///
    /// ```
    /// use tpt_telos_parser::parse;
    ///
    /// let modules = parse("module M { func foo(x: T) ; }").unwrap();
    /// assert_eq!(modules[0].items[0].func_name(), "foo");
    /// ```
    pub fn func_name(&self) -> String {
        match self {
            Item::Func(f) => f.name.clone(),
            Item::Invariant(i) => i.name.clone(),
            Item::Struct(s) => s.name.clone(),
            Item::Enum(e) => e.name.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invariant {
    pub name: String,
    /// One or more boolean constraint expressions that must always hold.
    pub constraints: Vec<Expr>,
    /// Source location of the `invariant` keyword.
    pub span: Span,
    /// Source location of each entry in `constraints`, index-aligned.
    pub constraint_spans: Vec<Span>,
}

// ---------------------------------------------------------------------------
// Struct / Enum definitions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<FieldDef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<VariantDef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDef {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantDef {
    pub name: String,
    pub fields: Vec<FieldDef>,
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    pub mutability: ParamMutability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamMutability {
    Immutable,
    Mutable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Func {
    /// Item-level attributes, e.g. `@eject(rust)` marking a hand-written,
    /// trusted opaque implementation whose contracts are enforced at the
    /// boundary.
    pub attributes: Vec<Attribute>,
    pub name: String,
    pub params: Vec<Param>,
    pub return_ty: Option<Type>,
    pub requires: Vec<Expr>,
    pub ensures: Vec<Expr>,
    pub body: Vec<Stmt>,
    /// True when the body was elided with `;` (intent-only). The agentic
    /// synthesizer is responsible for providing an implementation.
    pub elided: bool,
    /// Source location of the `func` keyword.
    pub span: Span,
    /// Source location of each entry in `requires`, index-aligned.
    pub requires_spans: Vec<Span>,
    /// Source location of each entry in `ensures`, index-aligned.
    pub ensures_spans: Vec<Span>,
}

impl Func {
    /// Whether this function is "ejected" -- its body is a trusted opaque block
    /// and only its outer contracts are enforced (at the boundary).
    ///
    /// # Examples
    ///
    /// ```
    /// use tpt_telos_parser::parse;
    ///
    /// let src = r#"
    ///     module M {
    ///         @eject(rust)
    ///         func compute(x: T) ;
    ///     }
    /// "#;
    /// let modules = parse(src).unwrap();
    /// let item = &modules[0].items[0];
    /// if let tpt_telos_parser::ast::Item::Func(f) = item {
    ///     assert!(f.is_ejected());
    ///     assert_eq!(f.eject_lang(), Some("rust"));
    /// }
    /// ```
    pub fn is_ejected(&self) -> bool {
        self.attributes.iter().any(|a| a.name == "eject")
    }

    /// The explicit eject target language, if given as `@eject(rust)` /
    /// `@eject(go)`.
    ///
    /// See [`Func::is_ejected`] for an example.
    pub fn eject_lang(&self) -> Option<&str> {
        for a in &self.attributes {
            if a.name == "eject" {
                for arg in &a.args {
                    if let Arg::Flag(f) = arg {
                        return Some(f.as_str());
                    }
                }
            }
        }
        None
    }

    /// Whether this function returns `Result<T, E>`.
    pub fn returns_result(&self) -> bool {
        matches!(&self.return_ty, Some(Type::Generic(name, _)) if name == "Result")
    }
}

// ---------------------------------------------------------------------------
// Statements
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// `mutate state { ... }`
    MutateState(Vec<Assign>),
    /// A bare assignment outside of `mutate state`.
    Assign(Assign),
    /// `let name [: Type] = expr;`
    Let(LetBinding),
    /// `if expr { stmts } [else { stmts }]`
    If(IfStmt),
    /// `match expr { pattern => stmts, ... }`
    Match(MatchStmt),
    /// `return [expr];`
    Return(Option<Expr>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetBinding {
    pub name: String,
    pub ty: Option<Type>,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfStmt {
    pub condition: Expr,
    pub then_body: Vec<Stmt>,
    pub else_body: Option<Vec<Stmt>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchStmt {
    pub scrutinee: Expr,
    pub arms: Vec<MatchArm>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Vec<Stmt>,
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    /// A literal integer pattern.
    Literal(i64),
    /// A binding pattern: `x` binds the value to name `x`.
    Var(String),
    /// A constructor pattern: `Variant(field, ...)` or `Variant` (unit).
    Constructor(String, Vec<Pattern>),
    /// A wildcard pattern: `_`.
    Wildcard,
}

// ---------------------------------------------------------------------------
// Assignments
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assign {
    pub target: Expr,
    pub op: AssignOp,
    pub value: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Int(i64),
    /// A bare identifier, e.g. `amount`.
    Var(String),
    /// A field access, e.g. `from.balance`.
    Field {
        base: String,
        field: String,
    },
    /// `old(expr)` -- the value of `expr` in the pre-state.
    Old(Box<Expr>),
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    Bin {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// A function call: `func(args)`.
    Call(CallExpr),
    /// A method call: `receiver.method(args)`.
    MethodCall(MethodCallExpr),
    /// An index expression: `receiver[index]`.
    Index(IndexExpr),
    /// An if expression: `if cond { a } else { b }`.
    If(IfExpr),
    /// A match expression: `match scrutinee { pattern => expr, ... }`.
    Match(MatchExpr),
    /// The try / error propagation operator: `expr?`.
    Try(Box<Expr>),
    /// A universal quantifier: `forall x: Type [in domain] { body }`.
    Forall(ForallExpr),
    /// An aggregate expression: `sum(expr)`, `min(a, b)`, etc.
    Aggregate(AggregateExpr),
    /// A range expression: `lo..hi` (exclusive upper bound).
    Range {
        lo: Box<Expr>,
        hi: Box<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallExpr {
    pub func: String,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodCallExpr {
    pub receiver: Box<Expr>,
    pub method: String,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexExpr {
    pub receiver: Box<Expr>,
    pub index: Box<Expr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfExpr {
    pub condition: Box<Expr>,
    pub then_expr: Box<Expr>,
    pub else_expr: Box<Expr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchExpr {
    pub scrutinee: Box<Expr>,
    pub arms: Vec<ExprMatchArm>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprMatchArm {
    pub pattern: Pattern,
    pub expr: Expr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForallExpr {
    pub var: String,
    pub var_ty: Type,
    pub domain: Option<Box<Expr>>,
    pub body: Box<Expr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregateExpr {
    pub op: AggregateOp,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregateOp {
    Sum,
    Min,
    Max,
    Count,
}

impl AggregateOp {
    pub fn op_name(&self) -> &'static str {
        match self {
            AggregateOp::Sum => "sum",
            AggregateOp::Min => "min",
            AggregateOp::Max => "max",
            AggregateOp::Count => "count",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}
