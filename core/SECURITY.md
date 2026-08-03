# Security Policy

BitVanes is a **zero-trust** data purification engine. Security and data
isolation are core design properties, not add-ons.

## Zero-telemetry guarantee

- Token counting is a pure-arithmetic **chars-per-token heuristic** — there is
  no tokenizer model, no vocab file, and **no network code**. No
  tokenization request ever leaves the process.
- The pipeline makes **no network calls** during parse, scrub, chunk, or
  Arrow assembly.
- In the browser, all processing happens in a sandboxed Web Worker; document
  bytes never leave the user's machine. (The browser path is being phased out
  — see `REBRAND.md`.)

## Reporting a vulnerability

Please report security issues privately by opening a **private security
advisory** at
https://github.com/BitVanes/core/security/advisories/new (do **not** open a
public issue). We aim to acknowledge within 48 hours and publish a fix with
a CVE and changelog entry once verified.

## Supported versions

Only the most recent minor release receives security fixes.

| Version | Supported |
|---------|-----------|
| 1.0.x   | ✅        |
| < 1.0   | ❌        |

## PII scrubbing scope

The built-in PII patterns are a best-effort first line of defense and are
**not** a substitute for a dedicated DLP/redaction product. Scrubbing runs
pre-tokenization so matches cannot be split across chunk boundaries, but
recall depends on input formatting (e.g., phone matching is E.164-only). Do
not rely on it as the sole control for regulated data.

## PDF redaction

`--pdf-mode redact` deletes PII text objects from the page-object tree and
draws a blackout; `--pdf-mode flatten` rasterizes the page to 300 DPI and drops
the text layer entirely. Both require a runtime `libpdfium`; if it is absent
the engine **fails closed** (`FeatureNotEnabled`) rather than emitting an
unredacted PDF. `--pdf-mode text-only` extracts and redacts text without
pdfium (no original PDF bytes are preserved on that path). Scanned/image-only
PDFs have no extractable text layer and are reported as invalid input (OCR is
out of scope).
