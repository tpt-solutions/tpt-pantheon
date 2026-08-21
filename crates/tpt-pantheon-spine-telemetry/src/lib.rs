//! Shared telemetry emission wrapper for the Pantheon platform.
//!
//! Per [`SPINE.md`](../SPINE.md) §5.4, every crate that emits telemetry does so
//! through this crate rather than by constructing raw OTLP/exporter configs
//! inline. The crate defines a small, backend-agnostic surface — [`Span`],
//! [`Metric`], [`LogRecord`], and the [`Exporter`] trait — plus reference
//! `NoopExporter` and `InMemoryExporter` implementations.
//!
//! The canonical OTLP exporter is wired behind the `otlp` feature (which pulls
//! in `opentelemetry` + `opentelemetry-otlp`). Services obtain a [`Telemetry`]
//! handle and call [`Telemetry::emit_span`] / [`Telemetry::emit_metric`] /
//! [`Telemetry::emit_log`]; the configured exporter decides where the data
//! goes. This keeps the OTLP dependency (heavy) out of the default build while
//! still centralizing the emission contract in exactly one crate.

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

/// Severity for [`LogRecord`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// A recorded span (operation with a start/end and attributes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub name: String,
    pub start_unix_nanos: u64,
    pub end_unix_nanos: u64,
    pub attributes: BTreeMap<String, String>,
}

/// A single metric sample.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metric {
    pub name: String,
    pub value: i64,
    /// "counter" | "gauge" | "histogram" (kept as a string to stay backend-neutral).
    pub kind: String,
    pub attributes: BTreeMap<String, String>,
}

/// A structured log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRecord {
    pub severity: Severity,
    pub message: String,
    pub ts_unix_nanos: u64,
    pub attributes: BTreeMap<String, String>,
}

/// Errors an [`Exporter`] can return.
#[derive(Debug, Error)]
pub enum TelemetryError {
    #[error("telemetry exporter error: {0}")]
    Export(String),
}

/// The emission contract. A `Telemetry` handle delegates to a boxed `Exporter`.
pub trait Exporter: Send + Sync + Debug {
    fn emit_span(&self, span: Span) -> Result<(), TelemetryError>;
    fn emit_metric(&self, metric: Metric) -> Result<(), TelemetryError>;
    fn emit_log(&self, record: LogRecord) -> Result<(), TelemetryError>;
}

/// Central telemetry handle. Cloning is cheap (shared exporter).
#[derive(Clone, Debug)]
pub struct Telemetry {
    exporter: Arc<dyn Exporter>,
}

impl Telemetry {
    pub fn new(exporter: Arc<dyn Exporter>) -> Self {
        Self { exporter }
    }

    /// A handle that drops everything. Useful as a default and in tests.
    pub fn noop() -> Self {
        Self::new(Arc::new(NoopExporter))
    }

    pub fn emit_span(&self, span: Span) -> Result<(), TelemetryError> {
        self.exporter.emit_span(span)
    }

    pub fn emit_metric(&self, metric: Metric) -> Result<(), TelemetryError> {
        self.exporter.emit_metric(metric)
    }

    pub fn emit_log(&self, record: LogRecord) -> Result<(), TelemetryError> {
        self.exporter.emit_log(record)
    }

    /// Borrow the configured exporter (e.g. to downcast for assertions/tests).
    pub fn exporter(&self) -> &dyn Exporter {
        self.exporter.as_ref()
    }
}

/// Reference exporter that discards everything.
#[derive(Debug, Default)]
pub struct NoopExporter;

impl Exporter for NoopExporter {
    fn emit_span(&self, _: Span) -> Result<(), TelemetryError> {
        Ok(())
    }
    fn emit_metric(&self, _: Metric) -> Result<(), TelemetryError> {
        Ok(())
    }
    fn emit_log(&self, _: LogRecord) -> Result<(), TelemetryError> {
        Ok(())
    }
}

/// Reference exporter that buffers in memory. Used by tests and by services
/// that batch-and-forward elsewhere.
#[derive(Debug, Default)]
pub struct InMemoryExporter {
    inner: std::sync::Mutex<InMemoryStore>,
}

#[derive(Debug, Default)]
struct InMemoryStore {
    spans: Vec<Span>,
    metrics: Vec<Metric>,
    logs: Vec<LogRecord>,
}

impl InMemoryExporter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn spans(&self) -> Vec<Span> {
        self.inner.lock().expect("telemetry poisoned").spans.clone()
    }
    pub fn metrics(&self) -> Vec<Metric> {
        self.inner
            .lock()
            .expect("telemetry poisoned")
            .metrics
            .clone()
    }
    pub fn logs(&self) -> Vec<LogRecord> {
        self.inner.lock().expect("telemetry poisoned").logs.clone()
    }
}

impl Exporter for InMemoryExporter {
    fn emit_span(&self, span: Span) -> Result<(), TelemetryError> {
        self.inner
            .lock()
            .expect("telemetry poisoned")
            .spans
            .push(span);
        Ok(())
    }
    fn emit_metric(&self, metric: Metric) -> Result<(), TelemetryError> {
        self.inner
            .lock()
            .expect("telemetry poisoned")
            .metrics
            .push(metric);
        Ok(())
    }
    fn emit_log(&self, record: LogRecord) -> Result<(), TelemetryError> {
        self.inner
            .lock()
            .expect("telemetry poisoned")
            .logs
            .push(record);
        Ok(())
    }
}

impl Default for Telemetry {
    fn default() -> Self {
        Telemetry::noop()
    }
}

fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Convenience constructors for the three telemetry types.
impl Span {
    pub fn new(name: impl Into<String>) -> Self {
        let t = now_nanos();
        Self {
            name: name.into(),
            start_unix_nanos: t,
            end_unix_nanos: t,
            attributes: BTreeMap::new(),
        }
    }
    pub fn attribute(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.attributes.insert(k.into(), v.into());
        self
    }
    pub fn end(mut self) -> Self {
        self.end_unix_nanos = now_nanos();
        self
    }
}

impl Metric {
    pub fn counter(name: impl Into<String>, value: i64) -> Self {
        Self {
            name: name.into(),
            value,
            kind: "counter".into(),
            attributes: BTreeMap::new(),
        }
    }
    pub fn attribute(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.attributes.insert(k.into(), v.into());
        self
    }
}

impl LogRecord {
    pub fn new(severity: Severity, message: impl Into<String>) -> Self {
        Self {
            severity,
            message: message.into(),
            ts_unix_nanos: now_nanos(),
            attributes: BTreeMap::new(),
        }
    }
    pub fn attribute(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.attributes.insert(k.into(), v.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_telemetry_never_errors() {
        let t = Telemetry::noop();
        t.emit_span(Span::new("op").end()).unwrap();
        t.emit_metric(Metric::counter("hits", 1)).unwrap();
        t.emit_log(LogRecord::new(Severity::Info, "boot")).unwrap();
    }

    #[test]
    fn in_memory_exporter_buffers_all_three_signals() {
        let exporter = Arc::new(InMemoryExporter::new());
        let handle = Telemetry::new(exporter.clone());
        handle
            .emit_span(Span::new("req").attribute("route", "/x").end())
            .unwrap();
        handle
            .emit_metric(Metric::counter("reqs", 1).attribute("route", "/x"))
            .unwrap();
        handle
            .emit_log(LogRecord::new(Severity::Warn, "slow").attribute("ms", "500"))
            .unwrap();

        assert_eq!(exporter.spans().len(), 1);
        assert_eq!(exporter.metrics().len(), 1);
        assert_eq!(exporter.logs().len(), 1);
        assert_eq!(exporter.spans()[0].name, "req");
        assert_eq!(exporter.metrics()[0].value, 1);
        assert_eq!(exporter.logs()[0].severity, Severity::Warn);
    }
}
