# tpt-pantheon-spine-audit-log

## Crate gate

1. **What does this solve that nothing else does?** It is the single, mandatory
   `AuditSink::append` contract that every Keystone-writing crate must route its
   auditable writes through, so auditability is enforced structurally instead of
   by convention. The closest existing thing is ad-hoc logging, which cannot
   prove a record was never altered after the fact.
2. **Which spine contracts does it satisfy, or except itself from?** Satisfies
   §5.2 (audit) by definition. Excepted from §5.1 (storage) and §5.4 (telemetry)
   as a pure policy/transport wrapper with no persistent state of its own —
   registered in SPINE.md §5 Boundary Register.
3. **Is this Pantheon, or a different program?** Pantheon-specific spine
   infrastructure; the contract (not the transport) is the point.

---

Shared audit log sink for the Pantheon platform.

Per [`SPINE.md`](../../SPINE.md) §5.2, any crate that mutates persistent state
depends on this crate and calls [`AuditSink::append`] in its write path. The
crate is deliberately tiny: it defines the `AuditEvent` shape, the `AuditSink`
trait (the `append` contract), and a couple of reference implementations used by
tests and lightweight services.

It also provides a tamper-evident hash chain: [`HashChainSink`] maintains a
`sha2` hash over each record plus the previous record's hash, and
[`verify_chain`] recomputes and validates the whole chain — so a single
corrupted record is detectable without trusting the store.

## Layout

- `AuditEvent` / `AuditEventBuilder` — the immutable record and its builder.
- `AuditSink` — the shared append contract.
- `InMemoryAuditSink` — reference sink for tests and forwarders.
- `HashChainSink` / `verify_chain` — tamper-evident chained logging.
