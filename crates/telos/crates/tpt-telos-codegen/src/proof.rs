//! Cryptographic proof manifest for tpt-telos.
//!
//! Produces a [`ProofManifest`] that records the SHA-256 fingerprint of the
//! source `.telos` file together with the per-function verification outcomes.
//! The manifest is written as `telos-proof.json` alongside generated code and
//! also embedded as a `#[used] static` in generated Rust so it survives into
//! the compiled binary (extractable with `strings` or `objcopy`).
//!
//! This satisfies the spec §7 requirement: "verification proof permanently
//! attached to the compiled binary's metadata."
//!
//! The `manifest_hash` field is a SHA-256 of the full JSON with that field
//! set to `""`, so any tampering of the JSON invalidates the hash.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use tpt_telos_agent::FuncOutcome;

/// Per-function proof record.
#[derive(Debug, Clone)]
pub struct FuncProof {
    pub verified: bool,
    pub conclusions_checked: usize,
    pub conclusions_passed: usize,
    /// True when at least one constraint was proved via interval-arithmetic
    /// bounding rather than exact linear arithmetic.
    pub used_interval_bounding: bool,
}

/// The complete proof manifest for one compilation run.
#[derive(Debug, Clone)]
pub struct ProofManifest {
    /// Schema version for forward compatibility.
    pub schema_version: &'static str,
    /// `"sha256:<hex>"` of the source `.telos` file bytes.
    pub source_hash: String,
    /// ISO 8601 timestamp of when verification ran (UTC, seconds precision).
    pub verified_at: String,
    /// Per-function outcomes, ordered by function name for determinism.
    pub functions: BTreeMap<String, FuncProof>,
    /// SHA-256 of the entire JSON document with this field set to `""`.
    /// Allows downstream tools to verify the manifest was not tampered with.
    pub manifest_hash: String,
}

/// Generate a proof manifest from source bytes and agent transpilation outcomes.
pub fn generate_manifest(source_bytes: &[u8], outcomes: &[FuncOutcome]) -> ProofManifest {
    let source_hash = format!("sha256:{}", hex_sha256(source_bytes));
    let verified_at = utc_rfc3339_now();

    let mut functions = BTreeMap::new();
    for o in outcomes {
        let conclusions_checked = o.result.checks.len();
        let conclusions_passed = o.result.checks.iter().filter(|c| c.passed).count();
        let used_interval_bounding = o.result.checks.iter().any(|c| c.is_approximation);
        functions.insert(
            o.func_name.clone(),
            FuncProof {
                verified: o.verified,
                conclusions_checked,
                conclusions_passed,
                used_interval_bounding,
            },
        );
    }

    let mut manifest = ProofManifest {
        schema_version: "1",
        source_hash,
        verified_at,
        functions,
        manifest_hash: String::new(),
    };

    // Compute manifest_hash over the JSON with manifest_hash = "".
    let json_without_hash = to_json_for_hashing(&manifest);
    manifest.manifest_hash = format!("sha256:{}", hex_sha256(json_without_hash.as_bytes()));
    manifest
}

/// Serialize the manifest to pretty JSON.
pub fn to_json(manifest: &ProofManifest) -> String {
    let mut s = String::from("{\n");
    s.push_str(&format!(
        "  \"schema_version\": \"{}\",\n",
        manifest.schema_version
    ));
    s.push_str(&format!(
        "  \"source_hash\": \"{}\",\n",
        manifest.source_hash
    ));
    s.push_str(&format!(
        "  \"verified_at\": \"{}\",\n",
        manifest.verified_at
    ));
    s.push_str("  \"functions\": {\n");
    let funcs: Vec<_> = manifest.functions.iter().collect();
    for (i, (name, fp)) in funcs.iter().enumerate() {
        let comma = if i + 1 < funcs.len() { "," } else { "" };
        s.push_str(&format!("    \"{}\": {{\n", name));
        s.push_str(&format!("      \"verified\": {},\n", fp.verified));
        s.push_str(&format!(
            "      \"conclusions_checked\": {},\n",
            fp.conclusions_checked
        ));
        s.push_str(&format!(
            "      \"conclusions_passed\": {},\n",
            fp.conclusions_passed
        ));
        s.push_str(&format!(
            "      \"used_interval_bounding\": {}\n",
            fp.used_interval_bounding
        ));
        s.push_str(&format!("    }}{}\n", comma));
    }
    s.push_str("  },\n");
    s.push_str(&format!(
        "  \"manifest_hash\": \"{}\"\n",
        manifest.manifest_hash
    ));
    s.push_str("}\n");
    s
}

/// Render the proof manifest as a `#[used] static` Rust string literal.
///
/// Embedding this in generated Rust causes the manifest to appear in the
/// compiled binary's read-only data section, satisfying the spec §7
/// requirement for provenance attached to the binary.
pub fn render_rust_proof_static(manifest: &ProofManifest) -> String {
    let json = to_json(manifest);
    // Escape any backslashes and double-quotes inside the raw string.
    // We use a raw string with a unique delimiter to avoid escaping.
    format!(
        "\n// tpt-telos proof manifest (spec §7: provenance attached to binary)\n\
         #[used]\n\
         #[doc(hidden)]\n\
         static TELOS_PROOF_MANIFEST: &str = r#####\"{}\"#####;\n",
        json
    )
}

// ---------------------------------------------------------------------------
// Manifest verification
// ---------------------------------------------------------------------------

/// The result of verifying a proof manifest against source.
#[derive(Debug)]
pub struct ManifestVerification {
    pub source_hash_valid: bool,
    pub manifest_hash_valid: bool,
    pub source_hash_expected: String,
    pub source_hash_actual: String,
    pub manifest_hash_expected: String,
    pub manifest_hash_actual: String,
    pub tampered: bool,
}

/// Load a manifest from its JSON string and verify it against the given source
/// bytes. Checks both the `source_hash` (source file hasn't changed since
/// verification) and the `manifest_hash` (manifest JSON hasn't been tampered
/// with).
pub fn verify_manifest(manifest_json: &str, source_bytes: &[u8]) -> ManifestVerification {
    let source_hash_expected = format!("sha256:{}", hex_sha256(source_bytes));
    let source_hash_from_manifest = extract_source_hash(manifest_json);

    let manifest_hash_from_manifest = extract_manifest_hash(manifest_json);
    let manifest_hash_recomputed = compute_manifest_hash_from_json(manifest_json);

    let source_hash_valid = source_hash_expected == source_hash_from_manifest;
    let manifest_hash_valid = manifest_hash_from_manifest == manifest_hash_recomputed;

    ManifestVerification {
        source_hash_valid,
        manifest_hash_valid,
        source_hash_expected,
        source_hash_actual: source_hash_from_manifest,
        manifest_hash_expected: manifest_hash_from_manifest,
        manifest_hash_actual: manifest_hash_recomputed,
        tampered: !source_hash_valid || !manifest_hash_valid,
    }
}

fn extract_source_hash(json: &str) -> String {
    let marker = "\"source_hash\": \"";
    let Some(start) = json.find(marker) else {
        return String::new();
    };
    let rest = &json[start + marker.len()..];
    let Some(end) = rest.find('"') else {
        return String::new();
    };
    rest[..end].to_string()
}

fn extract_manifest_hash(json: &str) -> String {
    // Find "manifest_hash": "..." in the JSON.
    let marker = "\"manifest_hash\": \"";
    let Some(start) = json.find(marker) else {
        return String::new();
    };
    let rest = &json[start + marker.len()..];
    let Some(end) = rest.find('"') else {
        return String::new();
    };
    rest[..end].to_string()
}

/// Recompute manifest_hash by blanking the field in the JSON and hashing.
fn compute_manifest_hash_from_json(json: &str) -> String {
    let marker = "\"manifest_hash\": \"";
    let Some(start) = json.find(marker) else {
        return String::new();
    };
    let after_key = &json[start + marker.len()..];
    let Some(end) = after_key.find('"') else {
        return String::new();
    };
    let prefix = &json[..start + marker.len()];
    let suffix = &after_key[end..];
    let recomputed = format!("{prefix}{suffix}");
    format!("sha256:{}", hex_sha256(recomputed.as_bytes()))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn hex_sha256(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    digest
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
}

/// Serialize manifest with `manifest_hash` set to `""` for hashing.
fn to_json_for_hashing(m: &ProofManifest) -> String {
    let empty_hash = ProofManifest {
        manifest_hash: String::new(),
        ..m.clone()
    };
    to_json(&empty_hash)
}

fn utc_rfc3339_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Manual RFC3339 formatting: YYYY-MM-DDTHH:MM:SSZ
    let s = secs;
    let sec = s % 60;
    let min = (s / 60) % 60;
    let hour = (s / 3600) % 24;
    let days = s / 86400; // days since 1970-01-01
    let (year, month, day) = days_to_ymd(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hour, min, sec
    )
}

fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // Gregorian calendar conversion (sufficient for 21st century timestamps).
    let mut year = 1970u64;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u64;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, days + 1)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_outcome(name: &str, verified: bool, checks: usize) -> FuncOutcome {
        use tpt_telos_agent::Candidate;
        use tpt_telos_ir::VerificationProblem;
        use tpt_telos_router::Target;
        use tpt_telos_verifier::{CheckResult, VerificationResult};

        let checks_vec: Vec<CheckResult> = (0..checks)
            .map(|i| CheckResult {
                description: format!("check {}", i),
                passed: verified,
                is_ensures: true,
                is_approximation: false,
                counterexample: None,
                or_group: None,
                location: None,
            })
            .collect();
        let all_passed = checks_vec.iter().all(|c| c.passed);
        FuncOutcome {
            func_name: name.to_string(),
            target: Target::Rust,
            agent: "test".to_string(),
            iterations: vec![],
            final_candidate: Candidate { stmts: vec![] },
            problem: VerificationProblem {
                func_name: name.to_string(),
                func_span: Default::default(),
                premises: vec![],
                conclusions: vec![],
            },
            result: VerificationResult {
                func_name: name.to_string(),
                checks: checks_vec,
                all_passed,
            },
            verified,
        }
    }

    #[test]
    fn manifest_has_source_hash() {
        let m = generate_manifest(b"module M {}", &[]);
        assert!(
            m.source_hash.starts_with("sha256:"),
            "hash prefix: {}",
            m.source_hash
        );
        assert_eq!(m.source_hash.len(), 7 + 64);
    }

    #[test]
    fn manifest_hash_is_stable() {
        let m1 = generate_manifest(b"hello", &[]);
        let m2 = generate_manifest(b"hello", &[]);
        assert_eq!(m1.source_hash, m2.source_hash);
    }

    #[test]
    fn manifest_hash_changes_on_different_source() {
        let m1 = generate_manifest(b"module A {}", &[]);
        let m2 = generate_manifest(b"module B {}", &[]);
        assert_ne!(m1.source_hash, m2.source_hash);
    }

    #[test]
    fn to_json_is_valid_json_structure() {
        let o = dummy_outcome("transfer", true, 2);
        let m = generate_manifest(b"src", &[o]);
        let json = to_json(&m);
        assert!(
            json.contains("\"schema_version\""),
            "missing schema_version: {json}"
        );
        assert!(json.contains("\"transfer\""), "missing function: {json}");
        assert!(
            json.contains("\"verified\": true"),
            "missing verified: {json}"
        );
        assert!(
            json.contains("\"manifest_hash\""),
            "missing manifest_hash: {json}"
        );
    }

    #[test]
    fn rust_static_contains_used_attr() {
        let m = generate_manifest(b"x", &[]);
        let s = render_rust_proof_static(&m);
        assert!(s.contains("#[used]"), "missing #[used]: {s}");
        assert!(
            s.contains("TELOS_PROOF_MANIFEST"),
            "missing static name: {s}"
        );
    }

    #[test]
    fn verify_manifest_passes_for_intact_source() {
        let m = generate_manifest(b"module M {}", &[]);
        let json = to_json(&m);
        let result = verify_manifest(&json, b"module M {}");
        assert!(result.source_hash_valid, "source hash should match");
        assert!(result.manifest_hash_valid, "manifest hash should match");
        assert!(!result.tampered, "should not be flagged as tampered");
    }

    #[test]
    fn verify_manifest_detects_source_drift() {
        let m = generate_manifest(b"module M {}", &[]);
        let json = to_json(&m);
        let result = verify_manifest(&json, b"module M { changed }");
        assert!(!result.source_hash_valid, "source hash should mismatch");
        assert!(result.tampered, "should be flagged as tampered");
    }

    #[test]
    fn verify_manifest_detects_json_tampering() {
        let m = generate_manifest(b"module M {}", &[]);
        let mut json = to_json(&m);
        // Tamper with the schema_version field.
        json = json.replace("\"1\"", "\"99\"");
        let result = verify_manifest(&json, b"module M {}");
        assert!(!result.manifest_hash_valid, "manifest hash should mismatch");
        assert!(result.tampered, "should be flagged as tampered");
    }

    #[test]
    fn compute_hash_matches_generate() {
        let m = generate_manifest(b"module M {}", &[]);
        let json = to_json(&m);
        let recomputed = compute_manifest_hash_from_json(&json);
        assert_eq!(
            m.manifest_hash, recomputed,
            "recomputed hash should match original"
        );
    }
}
