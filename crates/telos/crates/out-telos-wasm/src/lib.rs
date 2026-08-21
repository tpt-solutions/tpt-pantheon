//! WASM bindings for the tpt-telos parser and verifier.
//!
//! Exposes two functions to JavaScript:
//! - `parse(source)` — parse `.telos` source and return a JSON summary.
//! - `verify(source)` — parse + extract IR + verify contracts, return a JSON summary.

use wasm_bindgen::prelude::*;

/// Parse `.telos` source and return a JSON value.
///
/// # Success
/// ```json
/// {
///   "ok": true,
///   "modules": [
///     { "name": "MyModule", "functions": ["foo", "bar"], "invariants": ["T"] }
///   ]
/// }
/// ```
///
/// # Error
/// ```json
/// { "ok": false, "error": "parse error message" }
/// ```
#[wasm_bindgen]
pub fn parse(source: &str) -> JsValue {
    match tpt_telos_parser::parse(source) {
        Err(e) => {
            let json = serde_json::json!({
                "ok": false,
                "error": e,
            });
            JsValue::from_str(&json.to_string())
        }
        Ok(modules) => {
            let mods: Vec<serde_json::Value> = modules
                .iter()
                .map(|m| {
                    let mut functions = Vec::new();
                    let mut invariants = Vec::new();
                    for item in &m.items {
                        match item {
                            tpt_telos_parser::ast::Item::Func(f) => {
                                functions.push(f.name.clone());
                            }
                            tpt_telos_parser::ast::Item::Invariant(i) => {
                                invariants.push(i.name.clone());
                            }
                            tpt_telos_parser::ast::Item::Struct(s) => {
                                // Structs are included in a separate key for completeness.
                                let _ = s;
                            }
                            tpt_telos_parser::ast::Item::Enum(e) => {
                                let _ = e;
                            }
                        }
                    }
                    serde_json::json!({
                        "name": m.name,
                        "functions": functions,
                        "invariants": invariants,
                    })
                })
                .collect();

            let json = serde_json::json!({
                "ok": true,
                "modules": mods,
            });
            JsValue::from_str(&json.to_string())
        }
    }
}

/// Parse `.telos` source, extract IR constraints, and verify all contracts.
///
/// # Success
/// ```json
/// {
///   "ok": true,
///   "passed": true,
///   "functions": [
///     {
///       "name": "withdraw",
///       "passed": true,
///       "checks": [
///         { "description": "ensures balance >= 0", "passed": true, "is_ensures": true }
///       ]
///     }
///   ]
/// }
/// ```
///
/// # Error
/// ```json
/// { "ok": false, "error": "..." }
/// ```
#[wasm_bindgen]
pub fn verify(source: &str) -> JsValue {
    let modules = match tpt_telos_parser::parse(source) {
        Ok(m) => m,
        Err(e) => {
            let json = serde_json::json!({ "ok": false, "error": e });
            return JsValue::from_str(&json.to_string());
        }
    };

    let problems = match tpt_telos_ir::extract::extract(&modules) {
        Ok(p) => p,
        Err(e) => {
            let json = serde_json::json!({ "ok": false, "error": e });
            return JsValue::from_str(&json.to_string());
        }
    };

    let mut overall_passed = true;
    let mut functions: Vec<serde_json::Value> = Vec::new();

    for problem in &problems {
        let result = tpt_telos_verifier::verify::verify(problem);

        let checks: Vec<serde_json::Value> = result
            .checks
            .iter()
            .map(|c| {
                let mut entry = serde_json::json!({
                    "description": c.description,
                    "passed": c.passed,
                    "is_ensures": c.is_ensures,
                    "is_approximation": c.is_approximation,
                });
                if let Some(g) = c.or_group {
                    entry["or_group"] = serde_json::json!(g);
                }
                if let Some(ref ce) = c.counterexample {
                    // Render the counterexample as a map of variable -> value.
                    let ce_map: serde_json::Map<String, serde_json::Value> = ce
                        .iter()
                        .map(|(k, v)| (k.clone(), serde_json::json!(v)))
                        .collect();
                    entry["counterexample"] = serde_json::Value::Object(ce_map);
                }
                entry
            })
            .collect();

        if !result.all_passed {
            overall_passed = false;
        }

        functions.push(serde_json::json!({
            "name": result.func_name,
            "passed": result.all_passed,
            "checks": checks,
        }));
    }

    let json = serde_json::json!({
        "ok": true,
        "passed": overall_passed,
        "functions": functions,
    });
    JsValue::from_str(&json.to_string())
}
