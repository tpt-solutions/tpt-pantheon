//! # tpt-telos-uir-bridge
//!
//! The **Prover Bridge** for [`tpt-uir`](https://github.com/tpt-solutions/tpt-uir)
//! (Phase 4 of that project). It consumes a TPT-UIR [`Region`] — produced by the
//! `tpt-gpu` / `tpt-crucible` ingestion adapters — and formally proves that the
//! `tpt_memory.alloc` allocations inside each memory scope never exceed the
//! target hardware's physical-memory budget.
//!
//! The bridge reuses tpt-telos' own SMT core: the built-in Fourier-Motzkin
//! engine (sound over integers, no external dependencies) decides the linear
//! arithmetic of the allocation totals, and the optional `z3` feature routes
//! nonlinear sizes (e.g. a tensor with two symbolic dimensions) through the Z3
//! SMT solver for exact solving.
//!
//! ## Workflow
//!
//! 1. An ingestion adapter (in `tpt-gpu` / `tpt-crucible`) lowers its model to a
//!    TPT-UIR `Region` and serializes it to a `.tptuir` file (postcard).
//! 2. The liveness pass (`tpt-uir-dialects`) has already wrapped each
//!    alloc-bearing operation with `tpt_memory.scope_begin` / `tpt_memory.alloc`
//!    / `tpt_memory.scope_end`.
//! 3. `prove_memory_bounds` (or the `telos-uir-prove` CLI) walks the region,
//!    extracts every `mem.alloc` and its byte-size expression, and proves each
//!    scope's total stays within budget for all symbolic-dimension assignments.
//!
//! ```
//! # #[cfg(feature = "uir")] {
//! use tpt_telos_uir_bridge::{prove_memory_bounds, MemoryLimits, ProofResult};
//! use tpt_uir_core::ir::Region;
//! use tpt_uir_core::op_name::OpName;
//! use tpt_uir_core::attr::{Attribute, AttributeValue};
//! use tpt_uir_core::types::{Dimension, ScalarType, ShapeSpec, TensorType, Type};
//! use tpt_uir_core::{Block, Operation};
//!
//! let op = Operation {
//!     id: 1,
//!     op_name: OpName::new("tpt_memory", "alloc"),
//!     operands: vec![1],
//!     results: vec![],
//!     regions: vec![],
//!     attributes: vec![
//!         Attribute::string("scope", "s0"),
//!         Attribute {
//!             key: "tensor".into(),
//!             value: AttributeValue::Type(Type::Tensor(TensorType {
//!                 dtype: ScalarType::F32,
//!                 shape: Some(ShapeSpec { dimensions: vec![Dimension::Fixed(256)] }),
//!             })),
//!         },
//!     ],
//! };
//! let region = Region { blocks: vec![Block { arguments: vec![], operations: vec![op] }] };
//! let result = prove_memory_bounds(&region, &MemoryLimits::with_default(4096));
//! assert_eq!(result, ProofResult::Valid);
//! # }
//! ```
#![cfg_attr(not(feature = "uir"), allow(dead_code, unused_imports, unused_macros))]

#[cfg(feature = "uir")]
pub mod cli;
#[cfg(feature = "uir")]
pub mod expr;
#[cfg(feature = "uir")]
pub mod extract;
#[cfg(feature = "uir")]
pub mod prove;

#[cfg(feature = "uir")]
pub use cli::run_cli;
#[cfg(feature = "uir")]
pub use expr::SizeExpr;
#[cfg(feature = "uir")]
pub use extract::{
    extract_allocs, extract_bounded_dims, extract_scopes, extract_symbolic_dims, tensor_byte_expr,
    AllocInfo, ScopeInfo,
};
#[cfg(feature = "uir")]
pub use prove::{prove_memory_bounds, prove_tptuir_bytes, MemoryLimits, ProofResult};

#[cfg(feature = "uir")]
pub use tpt_uir_serde::{read_tptuir, write_tptuir};

/// Read a `.tptuir` file and prove its memory bounds in one call (requires the
/// `std` feature on `tpt-uir-serde`; enabled transitively by the `uir` feature).
#[cfg(feature = "uir")]
pub fn prove_tptuir_file(
    path: impl AsRef<std::path::Path>,
    limits: &MemoryLimits,
) -> Result<ProofResult, Box<dyn std::error::Error>> {
    let region = read_tptuir(path)?;
    Ok(prove_memory_bounds(&region, limits))
}
