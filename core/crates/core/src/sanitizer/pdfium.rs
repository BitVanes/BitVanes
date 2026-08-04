//! Coordinate-aware destructive PDF redaction backed by `pdfium-render`.
//!
//! `pdfium-render` binds to a Pdfium library **at runtime** (it does not ship
//! or download one). The build pipeline stays clean — no compile-time native
//! link — and the engine **fails closed**: if `libpdfium` is not available on
//! the host, [`redact_pdf`] returns [`BitVanesError::FeatureNotEnabled`] so the
//! caller can fall back to text-layer sanitization rather than silently
//! emitting an unredacted PDF.
//!
//! # Modes
//!
//! - [`PdfRedactMode::Redact`] (default): for each PII span, the bounding
//!   rectangle is computed from glyph boxes, every text page object overlapping
//!   that rectangle is **deleted** (the PII bytes are removed from the page
//!   object tree — they cannot be extracted or copied), and a solid black
//!   rectangle is drawn over the region.
//! - [`PdfRedactMode::Flatten`]: the page (with blackout rectangles drawn) is
//!   rendered to a 300 DPI bitmap, the entire vector/text object layer is
//!   **dropped**, and the rasterized image becomes the page's only object.
//!   This is the strongest guarantee: there is no text layer to recover.
//!
//! # Safety
//!
//! All FFI to the Pdfium C library is encapsulated inside the `pdfium-render`
//! dependency (which holds the `unsafe` blocks). This module calls only
//! `pdfium-render`'s safe API, so it contains no `unsafe` and the workspace
//! `#![deny(unsafe_code)]` remains in force.

use std::sync::OnceLock;

use pdfium_render::prelude::*;

use crate::error::{BitVanesError, Result};
use crate::pii::{PiiFinding, Scrubber};

/// Process-wide cache of whether a usable `libpdfium` was located. `pdfium-render`
/// enforces a single global binding, so we probe exactly once and reuse.
static PDFIUM_AVAILABLE: OnceLock<bool> = OnceLock::new();

/// The platform's libpdfium filename.
const fn pdfium_filename() -> &'static str {
    if cfg!(target_os = "macos") {
        "libpdfium.dylib"
    } else if cfg!(target_os = "windows") {
        "pdfium.dll"
    } else {
        "libpdfium.so"
    }
}

/// Searches the locations a bundled install puts `libpdfium` (next to the
/// binary, in a sibling `lib/`, or one dir up's `lib/` for a `bin/`+`lib/`
/// install layout). Returns the first existing candidate so the engine works
/// from a self-contained tarball/installer with no `PATH`/env setup.
fn bundled_pdfium() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    let name = pdfium_filename();
    let mut candidates = vec![
        exe_dir.join("lib").join(name),
        exe_dir.join(name),
    ];
    if let Some(up) = exe_dir.parent() {
        candidates.push(up.join("lib").join(name));
        candidates.push(up.join(name)); // flat bundle next to the binary's dir
    }
    candidates.into_iter().find(|p| p.exists())
}

/// Installs the global pdfium binding if a library can be found: prefer a
/// bundled sibling (so a tarball/installer "just works"), then the system
/// library. Never panics — returns `false` and the caller fails closed.
fn bind_pdfium() -> bool {
    if let Some(path) = bundled_pdfium() {
        if Pdfium::bind_to_library(path).is_ok() {
            return true;
        }
    }
    Pdfium::bind_to_system_library().is_ok()
}

/// Returns `true` if a usable runtime `libpdfium` is available. The probe runs
/// once and is cached; it never installs the global binding, so it is safe to
/// call before any redaction.
#[must_use]
pub fn pdfium_available() -> bool {
    *PDFIUM_AVAILABLE.get_or_init(bind_pdfium)
}

/// Returns a [`Pdfium`] handle, reusing the process-wide global binding. The
/// first call installs it; subsequent calls reuse it (fail-closed if the probe
/// found no library).
fn get_pdfium() -> Result<Pdfium> {
    if pdfium_available() {
        Ok(Pdfium::default())
    } else {
        Err(BitVanesError::FeatureNotEnabled(
            "pdfium runtime library not available; install libpdfium or use --pdf-mode text-only"
                .into(),
        ))
    }
}

/// Extracts and concatenates the text of every page (used for verifying that
/// PII did not survive redaction). Routes through the same global binding as
/// [`redact_pdf`].
///
/// # Errors
///
/// Returns [`BitVanesError::FeatureNotEnabled`] if pdfium is unavailable, or
/// [`BitVanesError::InvalidInput`] if the bytes are not a parseable PDF.
pub fn extract_pdf_text(bytes: &[u8]) -> Result<String> {
    let pdfium = get_pdfium()?;
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|e| BitVanesError::InvalidInput(format!("pdfium load failed: {e}")))?;
    let mut text = String::new();
    for index in 0..document.pages().len() {
        let page = document
            .pages()
            .get(index)
            .map_err(|e| BitVanesError::InvalidInput(format!("page access failed: {e}")))?;
        if let Ok(text_page) = page.text() {
            text.push_str(&text_page.all());
        }
    }
    Ok(text)
}

/// Redaction strategy for PDF output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PdfRedactMode {
    /// Delete overlapping text objects + draw black rectangles (default).
    #[default]
    Redact,
    /// Rasterize each page to 300 DPI, drop the vector/text layer entirely.
    Flatten,
}

impl PdfRedactMode {
    /// Parses a `--pdf-mode` string (`"redact"` | `"flatten"` | `"text-only"`).
    /// `"text-only"` returns `None` so callers can route to the text-layer path.
    ///
    /// # Errors
    ///
    /// Returns [`BitVanesError::InvalidInput`] for an unknown mode string.
    pub fn parse(s: &str) -> Result<Option<Self>> {
        match s.to_ascii_lowercase().as_str() {
            "redact" => Ok(Some(Self::Redact)),
            "flatten" => Ok(Some(Self::Flatten)),
            "text-only" | "text" => Ok(None),
            other => Err(BitVanesError::InvalidInput(format!(
                "unknown pdf-mode '{other}' (expected redact|flatten|text-only)"
            ))),
        }
    }
}

/// Metrics returned by a PDF sanitize run.
#[derive(Debug, Clone)]
pub struct PdfSanitizeResult {
    /// Number of pages processed.
    pub pages_processed: usize,
    /// Number of PII spans located (not all may be on every page).
    pub pii_matches_scrubbed: usize,
    /// Mode actually applied.
    pub mode_used: PdfRedactMode,
}

/// Destructively redacts PII in `bytes` and returns the sanitized PDF plus
/// metrics.
///
/// # Errors
///
/// - [`BitVanesError::FeatureNotEnabled`] if the runtime `libpdfium` cannot be
///   located (fail-closed — caller should fall back to text-layer).
/// - [`BitVanesError::InvalidInput`] if the bytes are not a parseable PDF or
///   pdfium reports a parse ambiguity.
pub fn redact_pdf(
    bytes: &[u8],
    scrubber: &Scrubber,
    mode: PdfRedactMode,
) -> Result<(Vec<u8>, PdfSanitizeResult)> {
    // Bind to a runtime Pdfium library. Fail-closed if none is available.
    let bindings = Pdfium::bind_to_system_library()
        .map_err(|e| BitVanesError::FeatureNotEnabled(
            format!(
                "pdfium runtime library not available ({e}); install libpdfium or fall back to --pdf-mode text-only"
            )
            .into(),
        ))?;
    let pdfium = Pdfium::new(bindings);

    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|e| BitVanesError::InvalidInput(format!("pdfium load failed: {e}")))?;

    let pages_len = document.pages().len();
    let mut total_matches = 0usize;

    for page_index in 0..pages_len {
        let mut page = document
            .pages()
            .get(page_index)
            .map_err(|e| BitVanesError::InvalidInput(format!("page access failed: {e}")))?;

        // Build the page text from pdfium chars and record each char's bounding
        // rect, so PII byte offsets (from the scrubber) map exactly to glyphs.
        let (page_text, char_rects) = collect_text_and_rects(&page)?;
        let (_redacted, _map, findings) = scrubber.scrub(&page_text);
        total_matches += findings.len();

        if findings.is_empty() {
            continue;
        }

        // Union char rects per finding → PII bounding rectangles.
        let pii_rects = pii_rects_from_findings(&page_text, &char_rects, &findings);

        match mode {
            PdfRedactMode::Redact => apply_redact(&mut page, &pii_rects)?,
            PdfRedactMode::Flatten => apply_flatten(&mut page, &pii_rects)?,
        }
    }

    let out = document
        .save_to_bytes()
        .map_err(|e| BitVanesError::InvalidInput(format!("pdfium save failed: {e}")))?;

    Ok((
        out,
        PdfSanitizeResult {
            pages_processed: usize::try_from(pages_len).unwrap_or(0),
            pii_matches_scrubbed: total_matches,
            mode_used: mode,
        },
    ))
}

/// Walks the page's text characters, returning `(text, char_rects)` where the
/// i-th entry of `char_rects` is the bounding rectangle of the i-th character
/// in `text`. Guarantees byte-offset-to-glyph alignment for the scrubber.
fn collect_text_and_rects(page: &PdfPage<'_>) -> Result<(String, Vec<PdfRect>)> {
    let text_page = page
        .text()
        .map_err(|e| BitVanesError::InvalidInput(format!("text access failed: {e}")))?;
    let mut text = String::new();
    let mut rects = Vec::new();
    for char_obj in text_page.chars().iter() {
        // unicode_char() returns Option<char>; substitute a space for glyphs we
        // cannot decode so offsets stay aligned with the rects vector.
        let ch = char_obj.unicode_char().unwrap_or(' ');
        let rect = char_obj
            .loose_bounds()
            .map_err(|e| BitVanesError::InvalidInput(format!("char bounds failed: {e}")))?;
        text.push(ch);
        rects.push(rect);
    }
    Ok((text, rects))
}

/// Maps each finding's `[offset_start, offset_end)` byte range (into `text`) to
/// the union of the corresponding char rectangles.
fn pii_rects_from_findings(
    text: &str,
    char_rects: &[PdfRect],
    findings: &[PiiFinding],
) -> Vec<PdfRect> {
    // Byte offset of each char so we can map a PII byte range → char indices.
    let char_byte_offsets: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
    let total = char_rects.len();

    findings
        .iter()
        .filter_map(|f| {
            let start = f.offset_start as usize;
            let end = f.offset_end as usize;
            let first = char_byte_offsets.iter().position(|&b| b >= start)?;
            let last = char_byte_offsets.iter().rposition(|&b| b < end)?;
            if first >= total || last >= total || last < first {
                return None;
            }
            let mut iter = char_rects[first..=last].iter();
            let mut acc = *iter.next()?;
            for r in iter {
                acc = union_rect(acc, *r);
            }
            Some(acc)
        })
        .collect()
}

/// Returns the smallest rectangle containing both `a` and `b`.
fn union_rect(a: PdfRect, b: PdfRect) -> PdfRect {
    PdfRect::new_from_values(
        a.bottom().value.min(b.bottom().value),
        a.left().value.min(b.left().value),
        a.top().value.max(b.top().value),
        a.right().value.max(b.right().value),
    )
}

/// Redact mode: delete every text page object overlapping a PII rect, then
/// draw a solid black rectangle over the rect. Deletions run in descending
/// index order so earlier indices stay valid.
fn apply_redact(page: &mut PdfPage<'_>, pii_rects: &[PdfRect]) -> Result<()> {
    for rect in pii_rects {
        let mut to_remove = Vec::new();
        for index in 0..page.objects().len() {
            let object = page
                .objects()
                .get(index)
                .map_err(|e| BitVanesError::InvalidInput(format!("object access: {e}")))?;
            if object.object_type() == PdfPageObjectType::Text && object.does_overlap_rect(rect) {
                to_remove.push(index);
            }
        }
        for index in to_remove.into_iter().rev() {
            page.objects_mut()
                .remove_object_at_index(index)
                .map_err(|e| BitVanesError::InvalidInput(format!("object remove: {e}")))?;
        }
        // Visual blackout over the (now empty) region.
        page.objects_mut()
            .create_path_object_rect(*rect, None, None, Some(PdfColor::BLACK))
            .map_err(|e| BitVanesError::InvalidInput(format!("rect draw: {e}")))?;
    }
    Ok(())
}

/// Flatten mode: draw blackout rects, render the page to a 300 DPI bitmap, then
/// strip the entire vector/text object layer and place the rasterized image as
/// the page's sole object — guaranteeing there is no recoverable text layer.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names
)]
fn apply_flatten(page: &mut PdfPage<'_>, pii_rects: &[PdfRect]) -> Result<()> {
    const DPI: f32 = 300.0;
    let page_w_pt = page.width().value;
    let page_h_pt = page.height().value;

    // 1. Draw black rectangles on the (still-vector) page so they bake into
    //    the rasterized image.
    for rect in pii_rects {
        page.objects_mut()
            .create_path_object_rect(*rect, None, None, Some(PdfColor::BLACK))
            .map_err(|e| BitVanesError::InvalidInput(format!("rect draw: {e}")))?;
    }

    // 2. Render to a 300 DPI bitmap. The temporary PdfBitmap borrow ends when
    //    `as_image()` returns an owned DynamicImage.
    let render_w = ((page_w_pt * DPI) / 72.0).round().max(1.0) as i32;
    let render_h = ((page_h_pt * DPI) / 72.0).round().max(1.0) as i32;
    let image = page
        .render(render_w, render_h, None)
        .map_err(|e| BitVanesError::InvalidInput(format!("render failed: {e}")))?
        .as_image()
        .map_err(|e| BitVanesError::InvalidInput(format!("bitmap decode: {e}")))?;

    // 3. Drop the entire vector/text object layer (descending order).
    let object_count = page.objects().len();
    for index in (0..object_count).rev() {
        page.objects_mut()
            .remove_object_at_index(index)
            .map_err(|e| BitVanesError::InvalidInput(format!("object strip: {e}")))?;
    }

    // 4. Place the rasterized image as the only page object, scaled to the page.
    let page_w = page.width();
    let page_h = page.height();
    page.objects_mut()
        .create_image_object(
            PdfPoints::ZERO,
            PdfPoints::ZERO,
            &image,
            Some(page_w),
            Some(page_h),
        )
        .map_err(|e| BitVanesError::InvalidInput(format!("image place: {e}")))?;

    Ok(())
}
