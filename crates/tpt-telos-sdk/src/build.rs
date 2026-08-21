//! Build step: write a [`Project`] to disk and shell `cargo build` / `go build`,
//! reading back the compiled Rust artifact bytes.
//!
//! These functions run external build tools; they do **not** run during the
//! headless `compile` / `compile_static` path (which is what the CI test suite
//! exercises). A build that runs but fails is reported as
//! `Ok(BuildOutput { success: false, .. })`; only a *missing* tool is an
//! `Err(SdkError::ToolNotFound)`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tpt_telos_codegen::project::RUST_CRATE_NAME;

use crate::{SdkError, VerifiedArtifact};

/// Outcome of compiling a [`Project`] to real artifacts.
#[derive(Debug, Clone)]
pub struct BuildOutput {
    /// True when every backend that was present built successfully.
    pub success: bool,
    /// Bytes of the compiled Rust artifact (`lib<RUST_CRATE_NAME>.rlib` / `.a`).
    /// `None` when no Rust backend was present, the build failed, or the
    /// artifact could not be read. Go yields no byte artifact.
    pub rust_artifact_bytes: Option<Vec<u8>>,
}

/// Write `artifact.project` under `out_dir` and build every present backend.
///
/// Missing `cargo`/`go` on `PATH` returns `Err(ToolNotFound)`; a build that
/// runs but fails returns `Ok(BuildOutput { success: false, .. })`.
pub fn compile_project(
    artifact: &VerifiedArtifact,
    out_dir: &Path,
) -> Result<BuildOutput, SdkError> {
    artifact.project.write(out_dir)?;

    let mut ok = true;
    let mut rust_bytes = None;

    if artifact.project.has_rust {
        let rust_dir = out_dir.join("rust");
        match std::process::Command::new("cargo")
            .arg("build")
            .arg("--manifest-path")
            .arg(rust_dir.join("Cargo.toml"))
            .status()
        {
            Ok(s) if s.success() => {
                rust_bytes = read_rust_artifact(&rust_dir);
            }
            Ok(_) => ok = false,
            Err(e) => {
                return Err(SdkError::ToolNotFound(format!("cargo: {e}")));
            }
        }
    }

    if artifact.project.has_go {
        match std::process::Command::new("go")
            .arg("build")
            .arg("./...")
            .current_dir(out_dir.join("go"))
            .status()
        {
            Ok(s) if s.success() => {}
            Ok(_) => ok = false,
            Err(e) => {
                return Err(SdkError::ToolNotFound(format!("go: {e}")));
            }
        }
    }

    Ok(BuildOutput {
        success: ok,
        rust_artifact_bytes: rust_bytes,
    })
}

/// Like [`compile_project`] but writes to a fresh temporary directory
/// (cleaned up afterwards) and returns the build result. Handy for callers
/// that only want the compiled artifact bytes without managing a directory.
pub fn compile_project_tempdir(artifact: &VerifiedArtifact) -> Result<BuildOutput, SdkError> {
    let dir = std::env::temp_dir().join(format!(
        "telos-sdk-{}-{}",
        std::process::id(),
        unique_nanos()
    ));
    std::fs::create_dir_all(&dir)?;
    let result = compile_project(artifact, &dir);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Locate and read the first compiled Rust artifact (`lib*.rlib` or `lib*.a`)
/// under `rust_dir/target/debug`.
fn read_rust_artifact(rust_dir: &Path) -> Option<Vec<u8>> {
    let target: PathBuf = rust_dir.join("target").join("debug");
    let entries = std::fs::read_dir(&target).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("lib")
            && (name.ends_with(".rlib") || name.ends_with(".a"))
            && name
                .strip_prefix("lib")
                .map(|s| s.starts_with(RUST_CRATE_NAME))
                .unwrap_or(false)
        {
            if let Ok(bytes) = std::fs::read(entry.path()) {
                return Some(bytes);
            }
        }
    }
    None
}

/// A monotonic-ish nanosecond timestamp for unique tempdir names.
fn unique_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_rust_artifact_missing_dir_is_none() {
        // A non-existent target dir yields no bytes without touching cargo.
        assert!(read_rust_artifact(Path::new("/nonexistent/rust")).is_none());
    }

    #[test]
    fn read_rust_artifact_reads_generated_rust_lib() {
        // A fake compiled artifact under target/debug is located and read back.
        let dir = std::env::temp_dir().join(format!("telos-sdk-readtest-{}", unique_nanos()));
        let target = dir.join("target").join("debug");
        std::fs::create_dir_all(&target).unwrap();
        let artifact = target.join(format!("lib{}.rlib", RUST_CRATE_NAME));
        std::fs::write(&artifact, b"fake-object-bytes").unwrap();

        let bytes = read_rust_artifact(&dir);
        assert_eq!(bytes.as_deref(), Some(&b"fake-object-bytes"[..]));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
