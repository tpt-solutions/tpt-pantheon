//! Error type for the tpt-telos SDK pipeline.

use std::fmt;

/// Errors that stop the pipeline from running. Verification *failure* is not an
/// error — see [`crate::VerifiedArtifact::all_verified`].
#[derive(Debug)]
pub enum SdkError {
    /// The `.telos` source could not be parsed.
    Parse(String),
    /// The agentic transpiler rejected the source (should be rare; usually a
    /// synthesizable-but-unverifiable `ensures` produces an `Err` here).
    Transpile(String),
    /// Code generation failed (e.g. an unsupported type crossed an FFI
    /// boundary).
    Codegen(String),
    /// An I/O error writing the project tree or reading back artifacts.
    Io(std::io::Error),
    /// `cargo` / `go` was required but not found on `PATH` (or failed to spawn).
    ToolNotFound(String),
}

impl fmt::Display for SdkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SdkError::Parse(s) => write!(f, "parse error: {s}"),
            SdkError::Transpile(s) => write!(f, "transpile error: {s}"),
            SdkError::Codegen(s) => write!(f, "codegen error: {s}"),
            SdkError::Io(e) => write!(f, "io error: {e}"),
            SdkError::ToolNotFound(s) => write!(f, "build tool not found: {s}"),
        }
    }
}

impl std::error::Error for SdkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SdkError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for SdkError {
    fn from(e: std::io::Error) -> Self {
        SdkError::Io(e)
    }
}
