# tpt-telos TODO

## Phase 1: The Core & The Parser (Months 1-3)
- [ ] Define the formal grammar for tpt-telos.
- [ ] Build the Rust-based parser and AST generator.
- [ ] Implement the basic constraint extraction (translating requires/ensures to Z3).
- [ ] **Milestone:** A CLI that can parse a .telos file and output a formal verification pass/fail.

## Phase 2: The Agentic Transpiler (Months 4-6)
- [ ] Integrate the LLM agent pipeline.
- [ ] Build the context router (deciding what goes to Rust vs. Go).
- [ ] Implement the "Verify -> Counter-example -> Rewrite" loop.
- [ ] **Milestone:** The compiler can take a simple .telos module and output mathematically verified, compiling Rust code.

## Phase 3: The Dual-Target & FFI (Months 7-9)
- [ ] Implement the Go backend generation.
- [ ] Build the automated FFI layer so generated Rust and Go code can call each other without manual glue code.
- [ ] **Milestone:** A fully functioning dual-backend compilation of a microservice.

## Phase 4: The "Eject" Hatch & DX (Months 10-12)
- [ ] Implement the two-way bridge for ejecting code to raw Rust/Go.
- [ ] Build the LSP server for IDE integration.
- [ ] **Milestone:** tpt-telos v1.0 release, ready for internal use in tpt-swarm and tpt-eve.
