# NEW CRATE TEMPLATE — the gate every future crate must clear

Before a crate in `tpt-pantheon` is allowed to exist, its README (or a tracking
doc) MUST open with the one-paragraph gate below, answered in prose. No
implementation work starts until the paragraph is written. This is the
friction-reducing filter from `spec.txt` §3: a crate that cannot answer these
three questions clearly should not be born.

Copy the block verbatim and fill in every field.

---

## Crate gate: `<crate-name>`

1. **What does this solve that nothing else does?**
   _(One or two sentences. Name the existing crate that is *closest* and state
   why it is insufficient rather than extending it.)_

2. **Which spine contracts does it satisfy, or which does it explicitly except
   itself from — and why?**
   _(Cite SPINE.md §1 storage, §5.2 audit, §5.3 sandbox, §5.4 telemetry. If it
   excepts itself from any, give the technical reason and add the exception to
   the Boundary Register in SPINE.md §5.)_

3. **Is this Pantheon, or a different program?**
   _(If the problem it solves is general-purpose and not specific to the
   Pantheon platform's thesis, it likely belongs in a separate repository. State
   the answer plainly.)_

---

### Example (filled, for `tpt-pantheon-spine-audit-log`)

1. **What does this solve that nothing else does?** It is the single,
   mandatory `AuditSink::append` contract that every Keystone-writing crate must
   route its auditable writes through, so auditability is enforced structurally
   instead of by convention.
2. **Which spine contracts does it satisfy, or except itself from?** Satisfies
   §5.2 (audit) by definition; excepted from §5.1 (storage) and §5.4 (telemetry)
   as a pure policy/transport wrapper with no persistent state of its own
   (registered in SPINE.md §5).
3. **Is this Pantheon, or a different program?** It is Pantheon-specific spine
   infrastructure; no general-purpose logging crate is a substitute because the
   contract is the point.
