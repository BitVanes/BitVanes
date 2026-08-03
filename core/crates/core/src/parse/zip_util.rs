//! Shared helpers for ZIP-based document formats (DOCX, PPTX, EPUB).
//!
//! All three are ZIP containers holding XML/XHTML parts. This module
//! provides the common `read_zip_entry` function so each parser only
//! implements its format-specific XML walking logic.

use std::io::Read;

use zip::ZipArchive;

use crate::error::{BitVanesError, Result};

/// Opens a ZIP archive from raw bytes and returns the decoded UTF-8 content
/// of the named entry.
///
/// # Errors
///
/// Returns [`BitVanesError::InvalidInput`] if the bytes are not a valid ZIP
/// archive, the entry is not found, or the entry is not valid UTF-8.
pub fn read_zip_entry(bytes: &[u8], entry_name: &str) -> Result<String> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| BitVanesError::InvalidInput(format!("invalid zip archive: {e}")))?;
    let mut file = archive
        .by_name(entry_name)
        .map_err(|e| BitVanesError::InvalidInput(format!("zip entry '{entry_name}': {e}")))?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)
        .map_err(|e| BitVanesError::InvalidInput(format!("zip read error: {e}")))?;
    Ok(buf)
}

/// Returns the raw bytes of a ZIP entry (for binary entries like images).
pub fn read_zip_entry_bytes(bytes: &[u8], entry_name: &str) -> Result<Vec<u8>> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| BitVanesError::InvalidInput(format!("invalid zip archive: {e}")))?;
    let mut file = archive
        .by_name(entry_name)
        .map_err(|e| BitVanesError::InvalidInput(format!("zip entry '{entry_name}': {e}")))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .map_err(|e| BitVanesError::InvalidInput(format!("zip read error: {e}")))?;
    Ok(buf)
}

/// Enumerates all entry names in the archive that start with `prefix`.
pub fn list_entries(bytes: &[u8], prefix: &str) -> Result<Vec<String>> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| BitVanesError::InvalidInput(format!("invalid zip archive: {e}")))?;
    let mut names = Vec::new();
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| BitVanesError::InvalidInput(format!("zip index error: {e}")))?;
        let name = entry.name().to_string();
        if name.starts_with(prefix) {
            names.push(name);
        }
    }
    Ok(names)
}
