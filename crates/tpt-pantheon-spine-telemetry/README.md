# tpt-pantheon-spine-telemetry

## Crate gate

1. **What does this solve that nothing else does?** It is the single, mandatory
   OTLP emission wrapper that every crate emits telemetry through, so the
   exporter surface is centralized in exactly one crate instead of being
   reconfigured inline per service. The closest existing thing is raw
   `opentelemetry` setup code duplicated across services.
2. **Which spine contracts does it satisfy, or except itself from?** Satisfies
   §5.4 (telemetry) by definition. Excepted from §5.1 (storage) and §5.2 (audit)
   as a pure emission wrapper with no persistent state of its own — registered in
   SPINE.md §5 Boundary Register.
3. **Is this Pantheon, or a different program?** Pantheon-specific spine
   infrastructure; centralizing the emission contract (not a specific backend) is
   the point.

---

Shared telemetry emission wrapper for the Pantheon platform.

Per [`SPINE.md`](../../SPINE.md) §5.4, every crate that emits telemetry does so
through this crate rather than by constructing raw OTLP/exporter configs inline.
The crate defines a small, backend-agnostic surface — `Span`, `Metric`,
`LogRecord`, and the `Exporter` trait — plus reference `NoopExporter` and
`InMemoryExporter` implementations.

The canonical OTLP exporter is wired behind the `otlp` feature (which pulls in
`opentelemetry` + `opentelemetry-otlp`). Services obtain a `Telemetry` handle and
call `emit_span` / `emit_metric` / `emit_log`; the configured exporter decides
where the data goes. This keeps the OTLP dependency (heavy) out of the default
build while still centralizing the emission contract in exactly one crate.

## Layout

- `Span` / `Metric` / `LogRecord` — the three telemetry signal types.
- `Exporter` — the emission contract.
- `Telemetry` — central handle (cheap to clone; shares one exporter).
- `NoopExporter` / `InMemoryExporter` — reference exporters for defaults and tests.
