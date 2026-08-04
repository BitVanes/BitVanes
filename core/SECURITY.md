# Security Policy

BitVanes is a **zero-trust** data purification engine. Security and data
isolation are core design properties, not add-ons.

## Zero-telemetry guarantee

- Token counting is a pure-arithmetic **chars-per-token heuristic** — there is
  no tokenizer model, no vocab file, and **no network code**. No
  tokenization request ever leaves the process.
- The pipeline makes **no network calls** during parse, scrub, chunk, or
  Arrow assembly. The Tier-2 NER sidecar (`bitvanes-nerd`) talks to the engine
  over a **local Unix-domain socket** — same host, no remote surface.
- The web dashboard performs no in-browser processing; it POSTs to the native
  daemon bound to `127.0.0.1` only. Document bytes never leave the host.

## Reporting a vulnerability

Please report security issues privately by opening a **private security
advisory** at
https://github.com/BitVanes/BitVanes/security/advisories/new (do **not** open
a public issue). We aim to acknowledge within 48 hours and publish a fix with
a CVE once verified.

## Supported versions

Only the most recent minor release receives security fixes.

| Version | Supported |
|---------|-----------|
| 1.0.x   | ✅        |
| < 1.0   | ❌        |

## PII scrubbing scope

BitVanes is a **purpose-built local DLP / redaction engine** — that is its
function, not an add-on. It is designed to be the data-purification control in
your pipeline: deterministic, air-gapped, and destructive (PII bytes are
removed, never painted over). Detection is two tiers:

- **Tier 1** (always on, in-process): regex + checksum patterns — email, SSN,
  US phone (E.164), credit card (Luhn), bank routing (ABA), street address
  (heuristic), AWS keys, GitHub PATs, JWTs. Scrubbing runs pre-tokenization so
  matches cannot be split across chunk boundaries; every redaction is recorded
  in the findings list.
- **Tier 2** (opt-in, paid): the `bitvanes-nerd` sidecar runs a local Int8
  BERT-NER (ConLL-2003 English: **PER / ORG / LOC / MISC**) so personal names,
  organizations, and locations are detected — closing the recall gap that pure
  regex cannot. It attaches automatically when a paid entitlement is present
  and the sidecar socket exists; the engine stays ML-free, and the sidecar
  re-verifies the license token offline (defense in depth).

Like every automated PII control, **recall is bounded**. Known limits to
validate against your data corpus:

- **Phone matching is E.164-only** (`+1…`); non-E.164 formats need a custom rule.
- **Street-address** detection is a low-base-confidence heuristic.
- **NER is the text path only** — destructive PDF redaction (`--pdf-mode`)
  runs Tier-1 today; PDF + NER lands next.
- **Scanned / image-only PDFs** have no text layer; OCR is out of scope.
- **NER is English (CoNLL-2003)**; multilingual models are a future swap
  (the wire contract and `CONLL_LABELS` are the seam — see `nerd/src/ner.rs`).

For regulated data, standard defense-in-depth applies: validate recall on your
corpus, and note the engine is **fail-closed** — if a configured Tier-2 sidecar
is unreachable mid-request, the pipeline refuses to emit rather than risk
leaking text the sidecar would have scrubbed.

## PDF redaction

`--pdf-mode redact` deletes PII text objects from the page-object tree and
draws a blackout; `--pdf-mode flatten` rasterizes the page to 300 DPI and drops
the text layer entirely. Both require a runtime `libpdfium`; if it is absent
the engine **fails closed** (`FeatureNotEnabled`) rather than emitting an
unredacted PDF. `--pdf-mode text-only` extracts and redacts text without
pdfium (no original PDF bytes are preserved on that path). Scanned/image-only
PDFs have no extractable text layer and are reported as invalid input (OCR is
out of scope).
