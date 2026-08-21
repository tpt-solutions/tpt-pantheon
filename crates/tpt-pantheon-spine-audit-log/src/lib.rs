//! Shared audit log sink for the Pantheon platform.
//!
//! Per [`SPINE.md`](../SPINE.md) §5.2, any crate that mutates persistent state
//! depends on this crate and calls [`AuditSink::append`] in its write path. The
//! crate is deliberately tiny: it defines the `AuditEvent` shape, the
//! [`AuditSink`] trait (the `append` contract), and a couple of reference
//! implementations used by tests and lightweight services. Persistence is
//! delegated to whatever backend a given service configures.

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

/// Outcome of the action an [`AuditEvent`] records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditOutcome {
    Allow,
    Deny,
    Error,
}

/// A single immutable audit record.
///
/// Cloning is cheap (small owned data) so sinks can buffer/forward events
/// without lifetime gymnastics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    pub id: String,
    pub ts_unix_nanos: u64,
    pub actor: String,
    pub action: String,
    pub target: String,
    pub outcome: AuditOutcome,
    pub meta: BTreeMap<String, String>,
}

impl AuditEvent {
    /// Start building an event. `id` should be unique; pass `None` to have one
    /// derived from the timestamp.
    pub fn builder(action: impl Into<String>, actor: impl Into<String>) -> AuditEventBuilder {
        AuditEventBuilder {
            id: None,
            ts: now_nanos(),
            actor: actor.into(),
            action: action.into(),
            target: String::new(),
            outcome: AuditOutcome::Allow,
            meta: BTreeMap::new(),
        }
    }
}

/// Builder for [`AuditEvent`] — keeps call sites readable and avoids forgetting
/// the timestamp.
pub struct AuditEventBuilder {
    id: Option<String>,
    ts: u64,
    actor: String,
    action: String,
    target: String,
    outcome: AuditOutcome,
    meta: BTreeMap<String, String>,
}

impl AuditEventBuilder {
    pub fn target(mut self, target: impl Into<String>) -> Self {
        self.target = target.into();
        self
    }

    pub fn outcome(mut self, outcome: AuditOutcome) -> Self {
        self.outcome = outcome;
        self
    }

    pub fn meta(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.meta.insert(key.into(), value.into());
        self
    }

    pub fn build(self) -> AuditEvent {
        AuditEvent {
            id: self
                .id
                .unwrap_or_else(|| format!("{:020}-{}", self.ts, self.action)),
            ts_unix_nanos: self.ts,
            actor: self.actor,
            action: self.action,
            target: self.target,
            outcome: self.outcome,
            meta: self.meta,
        }
    }
}

/// Errors a sink can return. Backends translate their own failures into this
/// shared type so callers stay backend-agnostic.
#[derive(Debug, Error)]
pub enum AuditError {
    #[error("audit sink backend error: {0}")]
    Backend(String),
    #[error("audit event rejected: {0}")]
    Rejected(String),
}

/// The audit contract: anything that writes persistent state routes its
/// auditable writes through `append`.
pub trait AuditSink: Send + Sync + Debug {
    fn append(&self, event: AuditEvent) -> Result<(), AuditError>;
}

/// Reference in-memory sink. Useful for tests and for services that forward
/// events over the wire (drain periodically into a real store).
#[derive(Debug, Default)]
pub struct InMemoryAuditSink {
    inner: std::sync::Mutex<Vec<AuditEvent>>,
}

impl InMemoryAuditSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot of everything appended so far (oldest first).
    pub fn events(&self) -> Vec<AuditEvent> {
        self.inner.lock().expect("audit sink poisoned").clone()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("audit sink poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl AuditSink for InMemoryAuditSink {
    fn append(&self, event: AuditEvent) -> Result<(), AuditError> {
        if event.action.is_empty() {
            return Err(AuditError::Rejected("action must not be empty".into()));
        }
        self.inner.lock().expect("audit sink poisoned").push(event);
        Ok(())
    }
}

fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

// --- Tamper-evident hash chain ---------------------------------------------
//
// `AuditEvent` itself is immutable and backend-agnostic; the chain is layered
// on top so a log can be proven append-only after the fact. Each record carries
// its own hash (over the previous hash plus its canonical content) in `meta`,
// which means a single bit of corruption anywhere in the chain is detectable by
// recomputing and comparing — without trusting the store.

use sha2::{Digest, Sha256};

const CHAIN_HASH_KEY: &str = "_chain_hash";
const CHAIN_PREV_KEY: &str = "_chain_prev";
const GENESIS_PREV: &str = "0";

/// Reasons a [`verify_chain`] / [`HashChainSink::verify`] call can fail.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChainError {
    #[error("record {0} is missing its chain hash")]
    MissingHash(String),
    #[error("record {0} hash mismatch: stored {2}, recomputed {1}")]
    HashMismatch(String, String, String),
    #[error("record {0} prev link broken: expected {1}, found {2}")]
    BrokenLink(String, String, String),
}

fn outcome_code(o: AuditOutcome) -> i32 {
    match o {
        AuditOutcome::Allow => 0,
        AuditOutcome::Deny => 1,
        AuditOutcome::Error => 2,
    }
}

/// Canonical hash of one record given the previous record's hash. The chain
/// keys themselves are excluded so the stored hash is reproducible from the
/// record's own content.
fn chain_hash(prev: &str, event: &AuditEvent) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prev.as_bytes());
    hasher.update(event.id.as_bytes());
    hasher.update(event.ts_unix_nanos.to_le_bytes());
    hasher.update(event.actor.as_bytes());
    hasher.update(event.action.as_bytes());
    hasher.update(event.target.as_bytes());
    hasher.update(outcome_code(event.outcome).to_le_bytes());
    let mut keys: Vec<&String> = event
        .meta
        .keys()
        .filter(|k| *k != CHAIN_HASH_KEY && *k != CHAIN_PREV_KEY)
        .collect();
    keys.sort();
    for k in keys {
        hasher.update(k.as_bytes());
        hasher.update(event.meta.get(k).unwrap().as_bytes());
    }
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Recompute and validate the hash chain over an ordered slice of records.
/// Returns `Ok(())` only if every record's stored hash matches its content and
/// its `prev` link matches the preceding record's hash.
pub fn verify_chain(events: &[AuditEvent]) -> Result<(), ChainError> {
    let mut prev = GENESIS_PREV.to_string();
    for event in events {
        let stored = event
            .meta
            .get(CHAIN_HASH_KEY)
            .ok_or_else(|| ChainError::MissingHash(event.id.clone()))?;
        let stored_prev = event
            .meta
            .get(CHAIN_PREV_KEY)
            .cloned()
            .unwrap_or_else(|| GENESIS_PREV.to_string());
        if stored_prev != prev {
            return Err(ChainError::BrokenLink(
                event.id.clone(),
                prev,
                stored_prev,
            ));
        }
        let recomputed = chain_hash(&stored_prev, event);
        if &recomputed != stored {
            return Err(ChainError::HashMismatch(
                event.id.clone(),
                recomputed,
                stored.clone(),
            ));
        }
        prev = stored.clone();
    }
    Ok(())
}

/// An [`AuditSink`] that wraps an [`InMemoryAuditSink`] and transparently
/// maintains a hash chain on every appended record. Read the accumulated log
/// via [`HashChainSink::events`] and prove it with [`HashChainSink::verify`]
/// (or the free [`verify_chain`] over any ordered slice).
#[derive(Debug, Default)]
pub struct HashChainSink {
    inner: InMemoryAuditSink,
}

impl HashChainSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot of the chained log (oldest first).
    pub fn events(&self) -> Vec<AuditEvent> {
        self.inner.events()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Prove the accumulated log is intact.
    pub fn verify(&self) -> Result<(), ChainError> {
        verify_chain(&self.inner.events())
    }
}

impl AuditSink for HashChainSink {
    fn append(&self, mut event: AuditEvent) -> Result<(), AuditError> {
        let prev = self
            .inner
            .events()
            .last()
            .and_then(|e| e.meta.get(CHAIN_HASH_KEY).cloned())
            .unwrap_or_else(|| GENESIS_PREV.to_string());
        event.meta.remove(CHAIN_PREV_KEY);
        event.meta.remove(CHAIN_HASH_KEY);
        let hash = chain_hash(&prev, &event);
        event.meta.insert(CHAIN_PREV_KEY.to_string(), prev);
        event.meta.insert(CHAIN_HASH_KEY.to_string(), hash);
        self.inner.append(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_stores_event_in_order() {
        let sink = InMemoryAuditSink::new();
        sink.append(
            AuditEvent::builder("write", "svc-a")
                .target("keystone:user/42")
                .outcome(AuditOutcome::Allow)
                .meta("field", "name")
                .build(),
        )
        .unwrap();
        sink.append(
            AuditEvent::builder("delete", "svc-a")
                .target("keystone:user/42")
                .outcome(AuditOutcome::Deny)
                .build(),
        )
        .unwrap();

        assert_eq!(sink.len(), 2);
        let events = sink.events();
        assert_eq!(events[0].action, "write");
        assert_eq!(events[0].outcome, AuditOutcome::Allow);
        assert_eq!(events[1].action, "delete");
        assert_eq!(events[1].outcome, AuditOutcome::Deny);
        assert_eq!(
            events[0].meta.get("field").map(String::as_str),
            Some("name")
        );
    }

    #[test]
    fn empty_action_is_rejected() {
        let sink = InMemoryAuditSink::new();
        let err = sink
            .append(AuditEvent::builder("", "svc-a").build())
            .unwrap_err();
        assert!(matches!(err, AuditError::Rejected(_)));
    }

    #[test]
    fn ids_are_unique_across_events() {
        let sink = InMemoryAuditSink::new();
        sink.append(AuditEvent::builder("read", "svc-b").build())
            .unwrap();
        sink.append(AuditEvent::builder("read", "svc-b").build())
            .unwrap();
        let ids: Vec<String> = sink.events().into_iter().map(|e| e.id).collect();
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn hash_chain_is_intact_when_untampered() {
        let sink = HashChainSink::new();
        sink.append(
            AuditEvent::builder("transfer", "svc-a")
                .target("keystone:account/1")
                .outcome(AuditOutcome::Allow)
                .meta("amount", "100")
                .build(),
        )
        .unwrap();
        sink.append(
            AuditEvent::builder("transfer", "svc-a")
                .target("keystone:account/2")
                .outcome(AuditOutcome::Deny)
                .meta("amount", "50")
                .build(),
        )
        .unwrap();
        sink.append(
            AuditEvent::builder("transfer", "svc-a")
                .target("keystone:account/3")
                .outcome(AuditOutcome::Allow)
                .build(),
        )
        .unwrap();

        assert_eq!(sink.len(), 3);
        assert!(sink.verify().is_ok());
        // The free function agrees over the same ordered slice.
        assert!(verify_chain(&sink.events()).is_ok());
    }

    #[test]
    fn hash_chain_detects_a_corrupted_record() {
        let sink = HashChainSink::new();
        sink.append(
            AuditEvent::builder("transfer", "svc-a")
                .target("keystone:account/1")
                .outcome(AuditOutcome::Allow)
                .meta("amount", "100")
                .build(),
        )
        .unwrap();
        sink.append(
            AuditEvent::builder("transfer", "svc-a")
                .target("keystone:account/2")
                .outcome(AuditOutcome::Deny)
                .build(),
        )
        .unwrap();

        // Tamper with the accumulated log: flip the outcome of record 0.
        let mut tampered = sink.events();
        tampered[0].outcome = AuditOutcome::Deny;
        assert!(matches!(
            verify_chain(&tampered),
            Err(ChainError::HashMismatch(_, _, _))
        ));
    }

    #[test]
    fn hash_chain_detects_a_broken_prev_link() {
        let sink = HashChainSink::new();
        sink.append(
            AuditEvent::builder("write", "svc-a")
                .target("keystone:user/42")
                .build(),
        )
        .unwrap();
        sink.append(
            AuditEvent::builder("delete", "svc-a")
                .target("keystone:user/42")
                .build(),
        )
        .unwrap();

        let mut tampered = sink.events();
        // Overwrite the second record's prev link with a wrong value.
        tampered[1]
            .meta
            .insert("_chain_prev".to_string(), "deadbeef".to_string());
        assert!(matches!(
            verify_chain(&tampered),
            Err(ChainError::BrokenLink(_, _, _))
        ));
    }
}
