//! Integration tests for the pdfium-backed destructive PDF redaction engine.
//!
//! These tests bind to a runtime `libpdfium`. If the library is not available
//! on the host, the destructive-mode tests self-skip (so `cargo test` stays
//! green in CI without pdfium); run them fully with the library on
//! `LD_LIBRARY_PATH`. The fail-closed test runs regardless, since a missing
//! library must ALSO surface as an error.

use bitvanes_core::pii::Scrubber;
use bitvanes_core::sanitizer::pdfium::{PdfRedactMode, redact_pdf};
use bitvanes_core::schema::{BuiltInPattern, ScrubProfile};
use pdfium_render::prelude::*;

/// The prebuilt PII fixture (SSN, email, valid-Luhn card, phone).
const FIXTURE: &[u8] = include_bytes!("fixtures/pii_sample.pdf");

fn ssn_scrubber() -> Scrubber {
    Scrubber::from_profile(&ScrubProfile {
        patterns: vec![
            BuiltInPattern::Ssn,
            BuiltInPattern::Email,
            BuiltInPattern::CreditCard,
        ],
        ..ScrubProfile::default()
    })
    .expect("scrubber compiles")
}

/// Returns Some(()) if a runtime pdfium library is available, else None.
fn pdfium_available() -> Option<()> {
    let bindings = Pdfium::bind_to_system_library();
    bindings.is_ok().then_some(())
}

/// Extracts all text from every page of a PDF (for asserting PII absence).
fn extract_all_text(bytes: &[u8]) -> String {
    let bindings = Pdfium::bind_to_system_library().expect("pdfium available");
    let pdfium = Pdfium::new(bindings);
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .expect("output PDF loads");
    let mut text = String::new();
    for index in 0..document.pages().len() {
        let page = document.pages().get(index).expect("page");
        text.push_str(&page.text().expect("text").all());
    }
    text
}

#[test]
fn redact_removes_pii_bytes_from_text_layer() {
    let Some(()) = pdfium_available() else {
        eprintln!("skip: libpdfium not on LD_LIBRARY_PATH");
        return;
    };
    let (out, result) = redact_pdf(FIXTURE, &ssn_scrubber(), PdfRedactMode::Redact)
        .expect("redact succeeds when pdfium is present");
    assert!(result.pages_processed >= 1);
    assert!(result.pii_matches_scrubbed >= 1, "should detect PII");

    let text = extract_all_text(&out);
    assert!(
        !text.contains("123-45-6789"),
        "SSN survived redaction in text layer: {text:?}"
    );
    assert!(
        !text.contains("leaker@example.com"),
        "email survived redaction: {text:?}"
    );
    assert!(
        !text.contains("4111111111111111"),
        "card survived redaction: {text:?}"
    );
}

#[test]
fn flatten_drops_text_layer_entirely() {
    let Some(()) = pdfium_available() else {
        eprintln!("skip: libpdfium not on LD_LIBRARY_PATH");
        return;
    };
    let (out, result) = redact_pdf(FIXTURE, &ssn_scrubber(), PdfRedactMode::Flatten)
        .expect("flatten succeeds when pdfium is present");
    assert!(result.pages_processed >= 1);
    // Output must be a valid PDF.
    assert!(
        out.starts_with(b"%PDF"),
        "flatten output is not a valid PDF header"
    );
    // A flattened page has no vector text layer: pdfium extracts no PII text.
    let text = extract_all_text(&out);
    assert!(
        !text.contains("123-45-6789"),
        "SSN present in flattened output: {text:?}"
    );
}

#[test]
fn malformed_pdf_fails_closed() {
    // Corrupt bytes: must error (either pdfium-missing or pdfium-parse-failure)
    // and must never return Ok with unredacted bytes.
    let garbage = b"%PDF-1.4\nnot a real pdf body\n%%EOF";
    let result = redact_pdf(garbage, &ssn_scrubber(), PdfRedactMode::Redact);
    assert!(
        result.is_err(),
        "malformed PDF must fail closed, got: {result:?}"
    );
}

#[test]
fn pdf_mode_parser_round_trips() {
    assert_eq!(
        PdfRedactMode::parse("redact").unwrap(),
        Some(PdfRedactMode::Redact)
    );
    assert_eq!(
        PdfRedactMode::parse("FLATTEN").unwrap(),
        Some(PdfRedactMode::Flatten)
    );
    assert_eq!(PdfRedactMode::parse("text-only").unwrap(), None);
    assert!(PdfRedactMode::parse("bogus").is_err());
}
