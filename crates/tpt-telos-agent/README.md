# tpt-telos-agent

**Agentic transpiler backend — Generate → Verify → Counter-example → Rewrite.**

Part of the [tpt-telos](https://github.com/tpt-solutions/tpt-telos) compiler workspace.

## Overview

`tpt-telos-agent` is the central orchestration layer of the tpt-telos pipeline. For each function it
runs a Generate → Verify → Counter-example → Rewrite loop: propose a candidate body, formally verify
it with `tpt-telos-verifier`, and if it fails extract a concrete counter-example that drives the next
rewrite attempt. The loop runs up to 8 iterations.

**`StaticAgent`** (always available) is a fully offline, deterministic synthesizer. It translates the
developer's body when one is present, or derives a body from the `ensures` clause when it isn't.

**`LlmAgent`** (behind the `llm` Cargo feature) calls a real LLM over an OpenAI-compatible or native
Anthropic HTTP API. Set `TELAS_LLM_KEY` + `TELAS_LLM_PROVIDER` at runtime.

## Usage

```rust
use tpt_telos_parser::parse;
use tpt_telos_agent::{StaticAgent, transpile_module};

let modules = parse(src).unwrap();
let agent = StaticAgent;
let outcomes = transpile_module(&modules[0], &agent).unwrap();

for outcome in &outcomes {
    println!("{}: verified={}", outcome.func_name, outcome.verified);
}
```

### LLM agent (requires `--features llm`)

```rust
use tpt_telos_agent::llm_agent::LlmAgent;

// Reads TELAS_LLM_PROVIDER / TELAS_LLM_KEY / TELAS_LLM_MODEL from env
let agent = LlmAgent::from_env().unwrap();
```

Supported providers via `TELAS_LLM_PROVIDER`: `openai` (default), `anthropic`, `ollama`,
`openrouter`, `grok`. Override the endpoint with `TELAS_LLM_URL`; cap tokens with
`TELAS_LLM_MAX_TOKENS` (Anthropic only).

## Key API

| Item | Description |
|------|-------------|
| `CodeAgent` trait | `name()`, `generate(spec)`, `rewrite(spec, prev, counterexample)` |
| `StaticAgent` | Offline deterministic agent; implements `CodeAgent` |
| `transpile_module(module, agent)` | Run the loop for every function in a module |
| `transpile_func(module, func_idx, agent)` | Run the loop for one function |
| `FuncOutcome` | Full result: `func_name`, `target`, `iterations`, `final_candidate`, `verified` |
| `Candidate` | A proposed function body: `stmts: Vec<Stmt>` |
| `LoopStep` | One iteration's record: `iteration`, `action`, `passed`, `counterexample` |

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your option.
