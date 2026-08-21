# tpt-telos Language Guide

This is the user-facing reference for the **tpt-telos** source language. It describes
the syntax, the type system, the contract model, and every language feature currently
supported by the verifier and the Rust/Go/Python backends.

The authoritative grammar is `crates/tpt-telos-parser/src/grammar.ebnf`. This guide is
written to be read top-to-bottom; for a hands-on walkthrough see
[`docs/TUTORIAL.md`](TUTORIAL.md), and for the CLI surface see
[`docs/CLI.md`](CLI.md).

---

## 1. Mental model

A tpt-telos program is a collection of **modules**. Each module is a unit of code that
compiles to one backend (Rust, Go, or Python). Every `func` in a module carries
**contracts** — `requires` (pre-conditions) and `ensures` (post-conditions) — that are
extracted to **QF_LRA** linear arithmetic and discharged by a self-contained
Fourier–Motzkin SMT-style solver. No external Z3 dependency is required for the common
case.

The verification story is simple:

- `requires e` — assumed true in the *pre-state*.
- `ensures e` — must be provable in the *post-state* from the `requires` clauses and the
  `mutate state` assignments.
- `invariant T { c }` — constraint `c` must hold for every value of type `T` at function
  entry and after every mutation.
- `old(e)` — the value of `e` in the pre-state (used to relate before/after).

Verification *failure* is a counterexample, not a panic. The compiler tells you which
clause failed and, when it can, a concrete assignment that breaks it.

---

## 2. Program structure

```
program  ::= module*

module   ::= attribute* "module" IDENT "{" item* "}"

item     ::= invariant
           | func
           | struct_def
           | enum_def
```

A module is tagged with one or more `@boundary(...)` attributes that pick the backend.
It contains invariants, functions, structs, and enums.

```telos
@boundary(cpu_bound)
module Wallet {
    invariant Account {
        balance >= 0
    }

    func deposit(acc: Account, amount: Int)
        requires amount > 0
        requires acc.balance >= 0
        ensures acc.balance == old(acc.balance) + amount
    {
        mutate state {
            acc.balance += amount
        }
    }
}
```

### Modules and files

A `.telos` file may contain multiple modules. `telos verify` verifies every function in
every module in the file. Cross-module examples live in `examples/cross_module.telos`.

---

## 3. Routing attributes (`@boundary`)

`@boundary(...)` on a module selects the target backend. Any Go flag wins; an
unannotated module defaults to **Rust**.

| Flag            | Backend | Note                                            |
|-----------------|---------|-------------------------------------------------|
| `cpu_bound`     | Rust    |                                                 |
| `zero_allocation` | Rust  |                                                 |
| `crypto`        | Rust    |                                                 |
| `real_time`     | Rust    | Rejected by `telos project --check --strict-rt` if routed to Go (GC is non-deterministic). |
| `network_io`    | Go      |                                                 |
| `high_concurrency` | Go   |                                                 |
| `distributed`   | Go      |                                                 |
| `high_latency`  | Go      |                                                 |
| `ml_training`   | Python  | Routes to the Python/JAX backend.               |
| `python`        | Python  |                                                 |
| `jax`           | Python  |                                                 |

```telos
@boundary(cpu_bound, zero_allocation)
module Ledger { /* -> Rust */ }

@boundary(network_io, high_concurrency)
module GatewayApi { /* -> Go */ }

@boundary(ml_training)
module Trainer { /* -> Python */ }
```

### `@state(...)` (storage class)

`@state(...)` forwards a storage class to the router:

- `persistent` → serde/JSON tags in the generated code.
- `ephemeral`  → stack-only storage.

```telos
@boundary(cpu_bound)
@state(persistent)
module Session { /* ... */ }
```

---

## 4. Types

```
type ::= IDENT                              // named type
       | IDENT "<" type ("," type)* ">"     // generic, e.g. Result<T, E>
       | "(" type ("," type)+ ")"           // tuple type
```

| Type         | Meaning                                                       |
|--------------|---------------------------------------------------------------|
| `Int`        | Integer (solver uses checked `i128`; degrades conservatively at `i64` extremes). |
| `Float32`    | IEEE-754 `f32`. Codegen'd to `f32`/`float32`; the IR currently tracks it as an integer constraint (QF_LRA). |
| `Float64`    | IEEE-754 `f64`. Same caveat as `Float32`.                     |
| `PositiveInt` | Built-in refinement type: `Int` with the implicit premise `value >= 0`. |
| Named types  | `struct`/`enum`/`invariant` names, declared in the same module or resolved globally (cross-module). |
| `Result<T,E>` | Generic return type; error propagation via `?`.             |
| `[T; N]`     | Fixed-size array in signatures; constant-index access
                (`arr[0]`) is allowed in contracts. Symbolic indices are rejected. |

There is **no implicit type coercion** and **no hidden allocation**: every operation is
named. `let` bindings infer their type from the right-hand side when no annotation is
given.

> **Not yet supported:** `String`/`bool` literals, symbolic array indices in contracts,
> array length invariants, and explicit cross-module type references. Invariant types
> *do* resolve across modules.

---

## 5. Functions

```
func ::= "func" IDENT "(" params? ")" [ "->" type ]
             clause*
             block | ";"
```

- `func f(x: T) -> Result<T, E>` declares a generic return type.
- A body-less function (`func f(...);`) is an **intent**: the agentic transpiler
  synthesizes an implementation from its `ensures` alone and verifies it
  (see `examples/intent.telos`).
- `mut param: T` marks a parameter as mutable (passed by reference).

### Clauses

```
clause ::= "requires" constraint
         | "ensures"  constraint
```

Multiple `requires` are conjoined (`&&`). Multiple `ensures` are each checked
independently.

### Statements

```
stmt ::= "mutate" "state" "{" assign* "}"          // the only place state changes
       | "let" IDENT [ ":" type ] "=" expr ";"
       | if_stmt
       | "match" expr "{" match_arm_stmt* "}"
       | "return" [ expr ] ";"
       | assign                                  // target (= | += | -=) expr
       | expr ";"                                // expression statement
```

`mutate state` is the **only** place that mutates fields. The verifier proves each
`ensures` against the assignments inside the `mutate state` block.

```telos
func transfer(from: Account, to: Account, amount: Int)
    requires amount > 0
    requires from.balance >= amount
    ensures from.balance == old(from.balance) - amount
    ensures to.balance   == old(to.balance)   + amount
{
    mutate state {
        from.balance -= amount
        to.balance   += amount
    }
}
```

---

## 6. Contracts in detail

### `requires` — pre-conditions

Assumed true at function entry. They narrow the solver's search space and are necessary
for contracts like "you may not overdraw an account".

### `ensures` — post-conditions

Provable from `requires` + the `mutate state` assignments. If a function has no
`ensures`, there is nothing to prove (but `invariant` checks still apply).

### `old(expr)` — pre-state reference

Refers to the value of `expr` *before* the mutation. This is how you state "the balance
went up by `amount`":

```telos
ensures acc.balance == old(acc.balance) + amount
```

### `invariant T { c }`

A property that must hold for *every* value of type `T`:

- at function entry (pre-state), and
- after every `mutate state` block (post-state).

Invariants are the backbone of state-machine safety (e.g. "a balance is never negative").
They are checked even when a function does not name the type in its own contracts.

### Disjunction (`||`)

`requires a || b` is normalized to **DNF** and verified as independent sub-problems. An
`ensures a || b` passes when **at least one** branch holds (tracked as a disjunction
group in the JSON output). See `examples/disjunction.telos`.

### Nonlinear contracts (interval bounding)

`x * y <= K` is verified via interval arithmetic **when both variables carry bounds** in
their `requires`. The result is tagged `[interval-bounded]` in the output to signal it
is an over-approximation. For exact integer arithmetic, build with `--features z3` and
pass `--solver z3`. See `examples/interval.telos`.

---

## 7. Expressions

```
expr  ::= orExpr
orExpr    ::= andExpr ("||" andExpr)*
andExpr   ::= cmpExpr ("&&" cmpExpr)*
cmpExpr   ::= addExpr [ cmpOp addExpr ]
cmpOp     ::= "==" | "!=" | "<" | "<=" | ">" | ">="
addExpr   ::= mulExpr (("+" | "-") mulExpr)*
mulExpr   ::= postfix (("*" | "/") postfix)*
postfix   ::= primary
            | postfix "(" arglist? ")"      // function / method call
            | postfix "." IDENT             // field access / method
            | postfix "[" expr "]"          // index
            | postfix "?"                   // error propagation

primary ::= INT
          | "old" "(" expr ")"
          | IDENT [ "." IDENT ]             // var or field path
          | "(" expr ")"
          | if_expr
          | match_expr
          | "forall" IDENT ":" type [ "in" expr ] "{" expr "}"
          | aggregate_call
          | "Ok" "(" expr ")"
          | "Err" "(" expr ")"
```

### Operators

| Category        | Operators                                   |
|-----------------|---------------------------------------------|
| Arithmetic      | `+  -  *  /` and unary `-`                  |
| Comparison      | `==  !=  <  <=  >  >=`                      |
| Logical and     | `&&` (in `requires`/`ensures`)              |
| Logical or      | `||` (via DNF normalization)                |
| Field path      | `wallet.balance`, `s.pressure`             |

### Control flow

```telos
// Statement form
if amount > 0 {
    mutate state { acc.balance += amount }
} else {
    return
}

// Expression form
let sign = if x > 0 { 1 } else { -1 };

// Pattern matching (exhaustiveness not enforced; a `_` wildcard is recommended)
match status {
    Active   => mutate state { count += 1 },
    Inactive => return,
    _        => return,
}
```

### `let` bindings

```telos
let x = acc.balance + amount;
let y: Int = compute();
```

In the IR a `let x = e;` becomes the equality `x == e`.

### Function & method calls

`func_name(args)` in an expression calls a function. In **specifications**, the call is
substituted with the function's post-conditions (modular verification). `receiver.method(args)`
is the method-call form. Both are resolved via callee `ensures` during modular
verification (see `examples/nested.telos`).

### Quantifiers (`forall`)

```telos
forall i: Int in 0..n {
    arr[i] >= 0
}
```

`forall` is parsed and codegen'd to runtime checks; **formal verification of quantifiers
is not yet performed**. Ranges are exclusive on the upper bound.

### Aggregates

`sum`, `min`, `max`, `count` over bounded ranges:

```telos
ensures total == sum(xs)
ensures max_val == max(a, b)
```

### Error propagation (`?`)

`expr?` desugars to a `match`/`return` for error propagation on `Result<T, E>`.

### Constructs

```telos
Ok(value)    // successful result
Err(error)   // error result
```

---

## 8. Data types: `struct` and `enum`

```telos
struct Point {
    x: Int,
    y: Int,
}

enum Status {
    Active,
    Inactive,
    Failed { code: Int },
}
```

- `struct Name { field: Type, ... }` declares a record type.
- `enum Name { Variant, Variant { field: Type }, ... }` declares a sum type.
- Field access is `value.field`; construction mirrors the declaration.
- Type inference also discovers types from field accesses, so you can use an
  `invariant` type name without an explicit `struct` declaration in many cases.

Patterns in `match`:

```
pattern ::= INT | "-" INT | IDENT [ "(" pattern ("," pattern)* ")" ] | "_"
```

---

## 9. The eject hatch (`@eject`)

`@eject` marks a function as a **trusted opaque block**. The compiler emits a generated
`*_impl`/`fImpl` function wrapped by a contract guard that enforces the `requires`/
`ensures` at runtime. Use it when you need hand-tuned code (e.g. calling a native
library) but still want the contract enforced at the boundary.

```telos
@eject
func process(c: Counter, by: Int)
    requires c.count >= by
    ensures c.count == old(c.count) - by
{
    mutate state { c.count -= by }
}
```

See `examples/eject.telos` and `telos eject` in [`docs/CLI.md`](CLI.md).

---

## 10. Float support

`Float32`/`Float64` are parsed and codegen'd to `f32`/`float32` and `f64`/`float64`. The
IR currently tracks them as integer constraints (QF_LRA), so contract verification over
floats is approximate. See `examples/float.telos`.

---

## 11. What is verified vs. what is codegen-only

| Feature                         | Verified | Notes |
|---------------------------------|:--------:|-------|
| `requires` / `ensures` (linear)| ✅ | QF_LRA via Fourier–Motzkin. |
| `old(expr)`                     | ✅ | |
| `invariant T { c }`             | ✅ | Checked at entry + post-mutate. |
| Disjunction (`||`)              | ✅ | DNF sub-problems. |
| Nonlinear (`x*y < K`)           | ⚠️ | Interval-bounded when both sides bounded; exact with `--solver z3`. |
| Floats                          | ⚠️ | Tracked as integer constraints. |
| `forall` / aggregates           | ❌ | Codegen to runtime checks only. |
| Array length invariants         | ❌ | Constant-index access in contracts only. |

---

## 12. Next steps

- Read [`docs/TUTORIAL.md`](TUTORIAL.md) for a hands-on first module.
- Browse `examples/START-HERE.telos` and `examples/README.md` for runnable fixtures.
- See [`docs/CLI.md`](CLI.md) for every `telos` subcommand, and
  [`docs/SDK.md`](SDK.md) for embedding tpt-telos in your own tooling.
