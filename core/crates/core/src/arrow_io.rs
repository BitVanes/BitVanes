//! Apache Arrow columnar output: schema, batch assembly, FFI export, IPC
//! streaming.
//!
//! This module is the contract between Rust and JavaScript: it defines the
//! [`Schema`] that `arrow-js-ffi` reads off the wasm heap, the release
//! registry that owns the lifetime of every exported [`RecordBatch`], and
//! the batch builder that converts [`ChunkSpec`]s into Arrow columns.
//!
//! # Schema stability
//!
//! Adding, removing, or reordering columns in [`output_schema`] is a
//! **breaking change** to both the npm and crates.io published artifacts
//! and MUST be gated by a semver-major version bump.
//!
//! [`RecordBatch`]: arrow::record_batch::RecordBatch
//! [`Schema`]: arrow::datatypes::Schema

pub mod batch;
pub mod ffi;

#[cfg(feature = "csv")]
pub mod csv;
#[cfg(feature = "ipc")]
pub mod ipc;

use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Fields, Schema, SchemaRef};

/// Returns the canonical output [`SchemaRef`] for a fully-processed
/// pipeline run (10 columns).
///
/// Column count and order are part of the public wire contract; changing
/// them is a semver-major break.
#[must_use]
pub fn output_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("chunk_index", DataType::UInt32, false),
        Field::new("chunk_id", DataType::Utf8, false),
        Field::new("text", DataType::Utf8, false),
        Field::new("token_count", DataType::UInt16, false),
        Field::new("source_path", DataType::Utf8, false),
        Field::new_list(
            "heading_path",
            Field::new("item", DataType::Utf8, true),
            true,
        ),
        Field::new(
            "section_kind",
            DataType::Dictionary(Box::new(DataType::Int8), Box::new(DataType::Utf8)),
            false,
        ),
        Field::new("char_offset_start", DataType::UInt32, false),
        Field::new("char_offset_end", DataType::UInt32, false),
        Field::new(
            "pii_metadata",
            DataType::List(Arc::new(Field::new_struct(
                "item",
                pii_struct_fields(),
                true,
            ))),
            true,
        ),
    ]))
}

/// Builds the nested `pii_metadata` column's inner struct type:
/// `Struct{entity: Utf8, confidence: Float32, offset_start: Int32,
/// offset_end: Int32, anchors: List<Utf8>}`.
fn pii_struct_fields() -> Fields {
    vec![
        Field::new("entity", DataType::Utf8, false),
        Field::new("confidence", DataType::Float32, false),
        Field::new("offset_start", DataType::Int32, false),
        Field::new("offset_end", DataType::Int32, false),
        Field::new_list("anchors", Field::new("item", DataType::Utf8, true), false),
    ]
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_schema_columns_match_contract() {
        let s = output_schema();
        assert_eq!(
            s.fields().len(),
            10,
            "column count must match the documented contract"
        );

        let names: Vec<&str> = s.fields().iter().map(|f| f.name().as_str()).collect();
        assert_eq!(
            names,
            [
                "chunk_index",
                "chunk_id",
                "text",
                "token_count",
                "source_path",
                "heading_path",
                "section_kind",
                "char_offset_start",
                "char_offset_end",
                "pii_metadata",
            ]
        );
    }

    #[test]
    fn output_schema_nullability_invariants() {
        let s = output_schema();

        // Document text and structural fields must never be null.
        for required in [
            "chunk_index",
            "chunk_id",
            "text",
            "token_count",
            "source_path",
            "section_kind",
            "char_offset_start",
            "char_offset_end",
        ] {
            assert!(
                !s.field_with_name(required).unwrap().is_nullable(),
                "{required} must be non-nullable"
            );
        }

        // heading_path may be null (top-level paragraphs have no ancestry).
        assert!(s.field_with_name("heading_path").unwrap().is_nullable());
        // pii_metadata may be null (chunk with no PII findings).
        assert!(s.field_with_name("pii_metadata").unwrap().is_nullable());
    }

    #[test]
    fn output_schema_pii_metadata_is_list_of_struct() {
        let s = output_schema();
        let f = s.field_with_name("pii_metadata").unwrap();
        match f.data_type() {
            DataType::List(inner) => {
                assert_eq!(inner.name(), "item");
                assert!(
                    matches!(inner.data_type(), DataType::Struct(_)),
                    "pii_metadata inner must be Struct, got {:?}",
                    inner.data_type()
                );
            }
            other => panic!("pii_metadata must be List<Struct>, got {other:?}"),
        }
    }

    #[test]
    fn output_schema_section_kind_is_int8_dictionary() {
        let s = output_schema();
        let f = s.field_with_name("section_kind").unwrap();
        assert!(
            matches!(f.data_type(), DataType::Dictionary(k, v)
                if **k == DataType::Int8 && **v == DataType::Utf8),
            "section_kind must be Dictionary<Int8, Utf8>, got {:?}",
            f.data_type()
        );
    }

    #[test]
    fn output_schema_is_consistent() {
        // Two calls must return structurally identical schemas.
        let a = output_schema();
        let b = output_schema();
        assert_eq!(a.as_ref(), b.as_ref());
    }
}
