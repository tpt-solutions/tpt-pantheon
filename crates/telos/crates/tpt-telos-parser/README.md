# tpt-telos-parser

**Lexer, parser, and AST for the tpt-telos language.**

Part of the [tpt-telos](https://github.com/tpt-solutions/tpt-telos) compiler workspace.

## Overview

`tpt-telos-parser` is the first stage of the tpt-telos pipeline. It takes raw `.telos` source text
and produces a typed AST (`Vec<Module>`) consumed by every downstream crate. The implementation is a
hand-written lexer and recursive-descent parser with no external parser-generator dependencies. All
AST node types are re-exported at the crate root for convenience.

The authoritative grammar lives at
[`src/grammar.ebnf`](https://github.com/tpt-solutions/tpt-telos/blob/master/crates/tpt-telos-parser/src/grammar.ebnf).

## Usage

```rust
use tpt_telos_parser::parse;

let src = r#"
    module Wallet {
        func deposit(balance: int, amount: int) -> int
            requires amount > 0
            ensures  result == balance + amount
        { balance + amount }
    }
"#;

let modules = parse(src).expect("parse error");
println!("{} module(s) parsed", modules.len());
```

## Key types

| Type | Description |
|------|-------------|
| `Module` | Top-level unit; holds a name, attributes, invariants, and functions |
| `Func` | Function definition with contracts (`requires`/`ensures`) and an optional body |
| `Expr` / `Stmt` | Expression and statement nodes |
| `Attribute` | `@boundary(...)` / `@state(...)` / `@eject(...)` metadata |
| `ParseError` | Structured error with source location |

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your option.
