# tpt-telos-prove

Standalone contradiction-checker binary for named constraint groups.

```
cargo install tpt-telos-prove
telos-prove groups.json
telos-prove groups.json --json --strict
```

Reads one or more named groups of linear-arithmetic constraints as JSON (from
a file argument or stdin) and reports pairwise/joint contradictions using the
same self-contained Fourier-Motzkin solver as the rest of the tpt-telos
pipeline — no Z3 or other external solver required.

## When to use this

Use `telos-prove` when you want to contradiction-check named constraint groups
but have no `.telos` source file. Typical uses:

- CI gate: verify that a set of alerting rules or SLO thresholds are mutually
  consistent before deploying them.
- Library consumers: translate domain invariants into `Constraint` objects and
  call `build_report` programmatically.
- Standalone install: `cargo install tpt-telos-prove` pulls only
  `tpt-telos-verifier`, `tpt-telos-ir`, and `tpt-telos-parser` — no codegen,
  agent, or LSP dependencies.

## Input schema

```json
{
  "groups": [
    {
      "label": "latency_ok",
      "constraints": [
        { "linear": { "terms": [["value", 1]], "constant": -200 }, "relation": "<=" }
      ]
    },
    {
      "label": "latency_critical",
      "constraints": [
        { "linear": { "terms": [["value", 1]], "constant": -500 }, "relation": ">=" }
      ]
    }
  ]
}
```

A bare top-level JSON array of groups is also accepted. `relation` is one of
`<=`, `>=`, `==`, `<`, `>`, `!=`. Each term in `terms` is a
`[variable, coefficient]` pair; `constant` is added directly to the linear
expression.

## Options

```
--json      Emit machine-readable JSON output
--strict    Exit non-zero on any contradiction or undecided result
--help      Print help
```

## Output

Human-readable (default):

```
overall: UNSATISFIABLE (the combined groups can never all hold)
  group latency_ok: consistent (on its own)
  group latency_critical: consistent (on its own)
  pairwise contradictions:
    latency_ok <-> latency_critical
```

JSON (`--json`):

```json
{
  "overall_unsat": true,
  "groups": [
    { "label": "latency_ok", "self_unsat": false },
    { "label": "latency_critical", "self_unsat": false }
  ],
  "contradictory_pairs": [
    ["latency_ok", "latency_critical"]
  ]
}
```
