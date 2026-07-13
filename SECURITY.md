# Security Policy

BitVanes is a **zero-trust** document ETL engine. Security and data
isolation are core design properties, not add-ons.

## Zero-telemetry guarantee

- BPE vocab files are embedded at compile time by `tiktoken-rs` via
  `include_str!`. The dependency contains **no network code** and exposes no
  feature to disable embedding. No tokenization request ever leaves the
  process.
- The pipeline makes **no network calls** during parse, scrub, chunk, or
  Arrow assembly.
- In the browser, all processing happens in a sandboxed Web Worker; document
  bytes never leave the user's machine.

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
| 0.2.x   | ✅        |
| < 0.2   | ❌        |

## PII scrubbing scope

The built-in PII patterns are a best-effort first line of defense and are
**not** a substitute for a dedicated DLP/redaction product. Scrubbing runs
pre-tokenization so matches cannot be split across chunk boundaries, but
recall depends on input formatting (e.g., phone matching is E.164-only). Do
not rely on it as the sole control for regulated data.
