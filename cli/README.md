# bitvanes-cli

Terminal tool for [BitVanes](https://bitvanes.com) — zero-trust local PII
purification. Direct, filter, and sanitize document streams before they reach
downstream APIs, databases, or AI tools. 100% on-premise; the engine makes no
network calls.

Four subcommands:

- **`scrub`** — batch-sanitize a file or directory (detect/redact PII + stats).
- **`filter`** — stream `stdin` → `stdout`, redacting PII in flight.
- **`daemon`** — local HTTP daemon on `127.0.0.1` for the dashboard / API.
- **`tui`** — interactive terminal UI.

## Install

### From release

Download the latest binary from
[GitHub Releases](https://github.com/BitVanes/cli/releases):

```bash
curl -L https://github.com/BitVanes/cli/releases/latest/download/bitvanes-x86_64-linux.tar.gz | tar xz
sudo mv bitvanes /usr/local/bin/
bitvanes --version
```

### From source

The CLI links `bitvanes-core` via a path dependency, so build it from the
monorepo layout (both repos checked out side by side):

```bash
git clone https://github.com/BitVanes/core.git
git clone https://github.com/BitVanes/cli.git
cd cli
cargo build --release
./target/release/bitvanes --help
```

## Usage

### Batch-sanitize a directory

```bash
# Sanitize every supported file under ./raw-documents/, mirror tree to ./clean/
bitvanes scrub ./raw-documents/ --out ./clean-documents/ --rules email,ssn,credit_card,street_address

# Single file → stdout
bitvanes scrub report.pdf --out - --rules email,ssn --redact mask
```

`scrub` prints a categorized summary on stderr:

```
BitVanes sanitization complete
  Files processed:    42
  Total bytes:        18,402,113
  PII instances:      318
  By category:
               email: 187
                 ssn: 64
        credit_card: 41
      street_address: 26
  Throughput:         142.7 MiB/s
```

`--redact` selects the output style: `placeholder` (default: `[REDACTED_SSN]`),
`mask` (`***********`), or `hash` (`[SHA256:8f3a9c12]`).

### Filter a stream

```bash
cat sensitive-stream.json | bitvanes filter --rules email,ssn,credit_card --mask-char "*" > clean.json
```

`filter` is bounded-memory: a rolling match window (default 1024 bytes) so a PII
token split across a read boundary is still redacted.

### Run the local daemon

```bash
bitvanes daemon --port 8080 --rules email,ssn,credit_card,phone,street_address
# optionally serve the built web dashboard:
bitvanes daemon --port 8080 --config Bitvanes.toml --dashboard-dir ../web/dist
```

The daemon binds to **127.0.0.1 only** (never `0.0.0.0`). Endpoints:

| Method | Path            | Body / Result                                          |
|--------|-----------------|--------------------------------------------------------|
| GET    | `/health`       | → `ok`                                                 |
| GET    | `/entitlement`  | → `{ status, plan, seats, allow_paid_features }`       |
| POST   | `/filter`       | request body (text) → sanitized text                   |
| POST   | `/scrub`        | `{ "text": "…" }` → `{ redacted, total, categories }`  |

Pass `--license-key bv1_…` (or set `BITVANES_LICENSE_KEY`) to surface a paid
plan in `/entitlement`. The signature-verification cloud backend is a stub;
see `src/entitlement.rs` for the wire-format contract.

When built with `--features dashboard`, the daemon also serves the embedded web
dashboard at `/` (no `--dashboard-dir` needed); drag-and-drop a `.txt`/`.md`/
`.json` file onto the dashboard to scrub it locally.

### Destructive PDF redaction

`scrub` redacts PDFs destructively by default (`--pdf-mode redact`): PII text
objects are deleted from the page-object tree and a black rectangle is drawn
over the region. `--pdf-mode flatten` rasterizes each page to 300 DPI and drops
the text layer entirely (strongest guarantee). Both require a runtime
`libpdfium`; if absent the command fails closed rather than emit unredacted
bytes. `--pdf-mode text-only` extracts and redacts text without pdfium.

```bash
bitvanes scrub report.pdf --out clean.pdf --rules email,ssn --pdf-mode flatten
```

### Interactive TUI

```bash
bitvanes tui
```

Press `?` inside the TUI for full keybinds (file browser → config → results).

## Configuration (`Bitvanes.toml`)

```toml
[pii]
patterns = ["email", "ssn", "credit_card", "street_address"]
min_confidence = 0.5
anchor_window = 7
report_only = ["phone"]
# Tier-2 personal-name gazetteer: customer-supplied phrases, matched
# case-insensitively on word boundaries.
names = ["Jane Doe", "Akhmad"]
use_generic_names = false   # opt-in bundled common-name starter list

[[pii.custom]]
name = "project_id"
regex = "\\bPROJECT-\\d+\\b"
replacement = "[PROJECT-ID]"

[output]
redaction = "placeholder"   # placeholder | mask | hash
mask_char = "*"
hash_hex_chars = 8
```

Pass it to any subcommand with `-c`/`--config Bitvanes.toml`.

## Output styles

| Style         | Example            | Notes                                  |
|---------------|--------------------|----------------------------------------|
| `placeholder` | `[REDACTED_SSN]`   | Default; typed per entity.             |
| `mask`        | `***********`      | Preserves match length.                |
| `hash`        | `[SHA256:8f3a9c12]`| Deterministic; enables offline joins.  |

## Built-in PII rules

`email`, `ssn`, `phone` (E.164), `credit_card` (Luhn), `routing_number` (ABA),
`street_address`, `aws_key`, `github_pat`, `jwt`. (Name detection requires a
local NER model — tracked under the `pii-model` feature.)

## Supported document formats

Markdown, plain text, HTML, JSON, PDF, DOCX, PPTX, XLSX, EPUB, RTF. Format is
auto-detected from the file extension. Scanned/image-only PDFs have no
extractable text layer and are reported as invalid input.

## License

MIT OR Apache-2.0.
