//! Walking a TPT-UIR [`Region`] to extract the facts the prover needs:
//! `tpt_memory.alloc` operations (with their `size_bytes` value handle and the
//! byte-size expression derived from the allocated tensor), the `Dimension::Symbolic`
//! variables, and `Dimension::Bounded` symbols with their maxima.

use std::collections::{BTreeSet, HashMap};

use tpt_uir_core::attr::AttributeValue;
use tpt_uir_core::ir::{Block, Operation, Region};
use tpt_uir_core::op_name::{TPT_MEMORY_ALLOC, TPT_MEMORY_SCOPE_BEGIN, TPT_MEMORY_SCOPE_END};
use tpt_uir_core::types::{Dimension, ScalarType, TensorType, Type};
use tpt_uir_core::ValueId;

use crate::expr::SizeExpr;

/// A single `tpt_memory.alloc` extracted from a region.
#[derive(Debug, Clone)]
pub struct AllocInfo {
    /// The `size_bytes` [`ValueId`] operand of the allocation op — the handle
    /// the producer used to name the buffer (used for reporting/tracing).
    pub size_value: ValueId,
    /// The `scope` attribute of the allocation (matches a `mem.scope_begin`/
    /// `mem.scope_end` `lifetime`).
    pub scope: String,
    /// The allocation's size in bytes, expressed symbolically over the region's
    /// dimension variables.
    pub byte_size: SizeExpr,
}

/// A `tpt_memory.scope_begin`/`scope_end` pair found in the region.
#[derive(Debug, Clone)]
pub struct ScopeInfo {
    pub lifetime: String,
    pub begins: usize,
    pub ends: usize,
}

/// Extract every `tpt_memory.alloc` op in the region (recursing into nested regions).
pub fn extract_allocs(region: &Region) -> Vec<AllocInfo> {
    let mut out = Vec::new();
    collect_allocs(region, &mut out);
    out
}

fn collect_allocs(region: &Region, out: &mut Vec<AllocInfo>) {
    for block in &region.blocks {
        collect_allocs_block(block, region, out);
    }
}

fn collect_allocs_block(block: &Block, region: &Region, out: &mut Vec<AllocInfo>) {
    for op in &block.operations {
        if op.op_name.to_string() == TPT_MEMORY_ALLOC {
            let size_value = op.operands.first().copied().unwrap_or(0);
            let scope = op
                .attributes
                .iter()
                .find(|a| a.key == "scope")
                .and_then(|a| match &a.value {
                    AttributeValue::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let byte_size = alloc_byte_size(op, block, region);
            out.push(AllocInfo {
                size_value,
                scope,
                byte_size,
            });
        }
        for nested in &op.regions {
            collect_allocs(nested, out);
        }
    }
}

/// Resolve the byte-size expression for a `mem.alloc` op.
///
/// Resolution order:
/// 1. an explicit `tensor`/`type` attribute on the alloc op carrying a
///    [`Type::Tensor`] (or a [`AttributeValue::Shape`]);
/// 2. the tensor type bound to the `size_bytes` value id among the enclosing
///    block's arguments (or any block argument in the region).
/// 3. otherwise `0` — the allocation contributes nothing to the budget (the
///    prover cannot account for it, so it fails safe toward a *smaller* total).
fn alloc_byte_size(op: &Operation, block: &Block, region: &Region) -> SizeExpr {
    for attr in &op.attributes {
        match &attr.value {
            AttributeValue::Type(Type::Tensor(t)) => return tensor_byte_expr(t),
            AttributeValue::Shape(s) => {
                return tensor_byte_expr(&TensorType {
                    dtype: ScalarType::U8,
                    shape: Some(s.clone()),
                });
            }
            _ => {}
        }
    }
    if let Some(value) = op.operands.first().copied() {
        if let Some(t) = block_arg_tensor(block, value) {
            return tensor_byte_expr(&t);
        }
        if let Some(t) = region_block_arg_tensor(region, value) {
            return tensor_byte_expr(&t);
        }
    }
    SizeExpr::const_(0)
}

fn block_arg_tensor(block: &Block, value: ValueId) -> Option<TensorType> {
    block
        .arguments
        .iter()
        .find(|(vid, _)| *vid == value)
        .and_then(|(_, ty)| match ty {
            Type::Tensor(t) => Some(t.clone()),
            _ => None,
        })
}

fn region_block_arg_tensor(region: &Region, value: ValueId) -> Option<TensorType> {
    for block in &region.blocks {
        if let Some(t) = block_arg_tensor(block, value) {
            return Some(t);
        }
        for op in &block.operations {
            for nested in &op.regions {
                if let Some(t) = region_block_arg_tensor(nested, value) {
                    return Some(t);
                }
            }
        }
    }
    None
}

/// Convert a tensor type into a symbolic byte-size expression:
/// `dtype_size * ∏ dimension`.
pub fn tensor_byte_expr(t: &TensorType) -> SizeExpr {
    let elem = t.dtype.size_bytes() as i64;
    let dims: Vec<Dimension> = match &t.shape {
        Some(s) => s.dimensions.clone(),
        None => Vec::new(),
    };
    let mut prod = SizeExpr::const_(elem.max(0));
    for d in dims {
        prod = SizeExpr::Mul(Box::new(prod), Box::new(dim_expr(&d)));
    }
    prod
}

fn dim_expr(d: &Dimension) -> SizeExpr {
    match d {
        Dimension::Fixed(n) => SizeExpr::const_(*n as i64),
        Dimension::Symbolic(s) => SizeExpr::var(s.clone()),
        Dimension::Bounded { symbol, .. } => SizeExpr::var(symbol.clone()),
    }
}

/// Extract all `Dimension::Symbolic` variable names across the region's tensor
/// types (unbounded symbolic dimensions only — `Bounded` symbols are returned by
/// [`extract_bounded_dims`]).
pub fn extract_symbolic_dims(region: &Region) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    collect_tensor_types(region, &mut |t| {
        if let Some(s) = &t.shape {
            for d in &s.dimensions {
                if let Dimension::Symbolic(name) = d {
                    out.insert(name.clone());
                }
            }
        }
    });
    out
}

/// Extract all `Dimension::Bounded` symbols with their maxima.
pub fn extract_bounded_dims(region: &Region) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    collect_tensor_types(region, &mut |t| {
        if let Some(s) = &t.shape {
            for d in &s.dimensions {
                if let Dimension::Bounded { symbol, max_value } = d {
                    if !out.iter().any(|(s2, _)| s2 == symbol) {
                        out.push((symbol.clone(), *max_value));
                    }
                }
            }
        }
    });
    out
}

/// Collect every tensor type appearing in the region (block-argument types and
/// `Type`/`Shape` attributes on operations).
fn collect_tensor_types(region: &Region, f: &mut dyn FnMut(&TensorType)) {
    for block in &region.blocks {
        for (_, ty) in &block.arguments {
            if let Type::Tensor(t) = ty {
                f(t);
            }
        }
        for op in &block.operations {
            for attr in &op.attributes {
                match &attr.value {
                    AttributeValue::Type(Type::Tensor(t)) => f(t),
                    AttributeValue::Shape(s) => f(&TensorType {
                        dtype: ScalarType::U8,
                        shape: Some(s.clone()),
                    }),
                    _ => {}
                }
            }
            for nested in &op.regions {
                collect_tensor_types(nested, f);
            }
        }
    }
}

/// Collect `mem.scope_begin`/`mem.scope_end` lifetimes for reporting and for
/// validating that every allocation's scope is actually opened/closed.
pub fn extract_scopes(region: &Region) -> Vec<ScopeInfo> {
    let mut out = Vec::new();
    let mut begins: HashMap<String, usize> = HashMap::new();
    let mut ends: HashMap<String, usize> = HashMap::new();
    collect_scope_marks(region, &mut begins, &mut ends);
    let lifetimes: BTreeSet<_> = begins.keys().chain(ends.keys()).cloned().collect();
    for lifetime in lifetimes {
        out.push(ScopeInfo {
            lifetime: lifetime.clone(),
            begins: begins.get(&lifetime).copied().unwrap_or(0),
            ends: ends.get(&lifetime).copied().unwrap_or(0),
        });
    }
    out
}

fn collect_scope_marks(
    region: &Region,
    begins: &mut HashMap<String, usize>,
    ends: &mut HashMap<String, usize>,
) {
    for block in &region.blocks {
        for op in &block.operations {
            let name = op.op_name.to_string();
            if name == TPT_MEMORY_SCOPE_BEGIN || name == TPT_MEMORY_SCOPE_END {
                let lifetime = op
                    .attributes
                    .iter()
                    .find(|a| a.key == "lifetime")
                    .and_then(|a| match &a.value {
                        AttributeValue::String(s) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                if name == TPT_MEMORY_SCOPE_BEGIN {
                    *begins.entry(lifetime).or_insert(0) += 1;
                } else {
                    *ends.entry(lifetime).or_insert(0) += 1;
                }
            }
            for nested in &op.regions {
                collect_scope_marks(nested, begins, ends);
            }
        }
    }
}
