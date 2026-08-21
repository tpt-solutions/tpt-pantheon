//! End-to-end test of the `telos-uir-prove` CLI logic. Gated behind `uir` so the
//! default `cargo test --workspace` (which has no sibling `tpt-uir` checkout)
//! compiles an empty test instead of requiring the cross-repo dependency.
#![cfg(feature = "uir")]

use tpt_telos_uir_bridge::{run_cli, write_tptuir};
use tpt_uir_core::attr::{Attribute, AttributeValue};
use tpt_uir_core::op_name::OpName;
use tpt_uir_core::types::{Dimension, ScalarType, ShapeSpec, TensorType, Type};
use tpt_uir_core::{Block, Operation, Region};

fn region_with(ops: Vec<Operation>) -> Region {
    Region {
        blocks: vec![Block {
            arguments: vec![],
            operations: ops,
        }],
    }
}

fn alloc(id: u32, dims: Vec<Dimension>, scope: &str) -> Operation {
    Operation {
        id,
        op_name: OpName::new("tpt_memory", "alloc"),
        operands: vec![id],
        results: vec![],
        regions: vec![],
        attributes: vec![
            Attribute::string("scope", scope.to_string()),
            Attribute {
                key: "tensor".to_string(),
                value: AttributeValue::Type(Type::Tensor(TensorType {
                    dtype: ScalarType::F32,
                    shape: Some(ShapeSpec { dimensions: dims }),
                })),
            },
        ],
    }
}

fn write_tmp(name: &str, region: &Region) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(name);
    write_tptuir(&path, region).expect("write .tptuir");
    path
}

#[test]
fn cli_reports_valid() {
    // 1024 + 2048 = 3072 bytes, limit 4096 -> valid.
    let region = region_with(vec![
        alloc(1, vec![Dimension::Fixed(256)], "s0"),
        alloc(2, vec![Dimension::Fixed(512)], "s0"),
    ]);
    let path = write_tmp("telos_uir_cli_valid.tptuir", &region);

    let code = run_cli([
        "telos-uir-prove",
        path.to_str().unwrap(),
        "--default-limit",
        "4096",
    ]);
    assert_eq!(code, 0);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn cli_reports_counterexample() {
    // 1024 + 2048 = 3072 bytes, limit 3000 -> over budget.
    let region = region_with(vec![
        alloc(1, vec![Dimension::Fixed(256)], "s0"),
        alloc(2, vec![Dimension::Fixed(512)], "s0"),
    ]);
    let path = write_tmp("telos_uir_cli_ce.tptuir", &region);

    let code = run_cli([
        "telos-uir-prove",
        path.to_str().unwrap(),
        "--default-limit",
        "3000",
    ]);
    assert_eq!(code, 1);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn cli_reports_inconclusive_for_nonlinear() {
    // Two symbolic dims -> nonlinear size; without Z3 -> inconclusive.
    let region = region_with(vec![alloc(
        1,
        vec![
            Dimension::Symbolic("n".to_string()),
            Dimension::Symbolic("m".to_string()),
        ],
        "s0",
    )]);
    let path = write_tmp("telos_uir_cli_inc.tptuir", &region);

    let code = run_cli([
        "telos-uir-prove",
        path.to_str().unwrap(),
        "--default-limit",
        "1000",
    ]);
    assert_eq!(code, 2);

    let _ = std::fs::remove_file(&path);
}
