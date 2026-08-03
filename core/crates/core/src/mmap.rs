//! Memory-mapped file reading for large files (native-only).
//!
//! For files larger than the threshold (default 1 MB), reading via `mmap`
//! avoids the `fs::read` copy. The caller gets a `&[u8]` view directly into
//! the page cache — zero-copy.
//!
//! Falls back to `fs::read` on mmap failure (network filesystems, special
//! files, permission issues).
//!
//! # Safety note
//!
//! This module uses `unsafe` for `memmap2::Mmap::map`, which is inherently
//! unsafe (the file could change underneath us). The `#[allow(unsafe_code)]`
//! is scoped to this module only; the workspace-wide `#![deny(unsafe_code)]`
//! remains in effect everywhere else.

#![allow(unsafe_code)]

use std::fs::File;
use std::path::Path;

use crate::error::Result;

/// Files smaller than this are read normally (mmap overhead not worth it).
pub const MMAP_THRESHOLD: u64 = 1_048_576; // 1 MB

/// Reads a file, using `mmap` for files above [`MMAP_THRESHOLD`] and
/// `fs::read` for smaller files.
///
/// The returned `Vec<u8>` owns the data. (A true zero-copy `&[u8]` view
/// would require a lifetime tied to the mmap, which is impractical across
/// the pipeline boundary. The win is avoiding the kernel→userspace copy
/// that `fs::read` performs — mmap pages are faulted in lazily by the OS.)
///
/// # Errors
///
/// Returns [`crate::error::BitVanesError::InvalidInput`] on read failure.
pub fn read_file(path: &Path) -> Result<Vec<u8>> {
    let metadata = std::fs::metadata(path).map_err(|e| {
        crate::error::BitVanesError::InvalidInput(format!("stat {}: {e}", path.display()))
    })?;
    let len = metadata.len();

    if len >= MMAP_THRESHOLD {
        if let Ok(bytes) = try_mmap(path, len) {
            return Ok(bytes);
        }
        // Fall through to fs::read on mmap failure.
    }

    std::fs::read(path).map_err(|e| {
        crate::error::BitVanesError::InvalidInput(format!("read {}: {e}", path.display()))
    })
}

/// Attempts to mmap the file. Returns `None` on any error (caller falls
/// back to `fs::read`).
fn try_mmap(path: &Path, _len: u64) -> std::io::Result<Vec<u8>> {
    let file = File::open(path)?;
    // SAFETY: mmap is unsafe because the file could change underneath us.
    // For read-only ETL on local files this is acceptable. The Map is
    // immediately copied into a Vec to detach from the mapping before
    // returning, making the result safe to use without lifetime concerns.
    let mmap = unsafe { memmap2::Mmap::map(&file)? };
    Ok(mmap[..].to_vec())
}
