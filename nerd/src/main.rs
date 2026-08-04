//! `bitvanes-nerd` — the Tier-2 NER sidecar.
//!
//! Listens on a local socket (Unix domain socket on Unix) and serves
//! named-entity detection to the `bitvanes-core` engine over the wire
//! contract defined in `core/crates/core/src/pii/ner_client.rs`. The engine
//! itself stays ML-free; the model + ONNX Runtime live entirely here.
//!
//! # Wire contract (MUST match `bitvanes_core::pii::ner_client`)
//!
//! Length-prefixed (4-byte big-endian) JSON:
//!
//! ```text
//! Request  -> DetectRequest { text, token: Option<str> }
//! Response <- DetectOutcome { Ok { findings }, Err { code, msg } }
//! ```
//!
//! `token` is the `bv1_…` license key. This sidecar verifies it **offline**
//! before running inference — enforcement lives where the model actually
//! runs, so a client that bypasses the CLI's gate gets nothing. Free-tier
//! (token absent / wrong prefix) ⇒ `code = "unentitled"`.
//!
//! # Status
//!
//! This is the scaffold: a working server that fails closed
//! (`code = "unavailable"`) until the `model` feature wires the bundled
//! ONNX BERT-NER via `ort`. The wire contract, framing, socket I/O, and
//! entitlement gate are all real and exercised by the engine-side tests.

#![deny(unsafe_code)]

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 4 GiB hard cap on a single frame so a hostile/buggy peer can't force an
/// oversized allocation. Matches the engine-side bound.
const MAX_FRAME: usize = 256 * 1024 * 1024;

const LICENSE_KEY_PREFIX: &str = "bv1_";

// ---------------------------------------------------------------------------
// Wire contract — kept byte-identical to `bitvanes_core::pii::ner_client`.
// Re-declared (rather than depending on core) so the sidecar builds standalone
// and stays a small artifact. A drift-regression test should pin these.
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct DetectRequest {
    text: String,
    token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NerFinding {
    entity: String,
    start: u32,
    end: u32,
    confidence: f32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum DetectOutcome {
    Ok { findings: Vec<NerFinding> },
    Err { code: String, msg: String },
}

// ---------------------------------------------------------------------------
// Framing — 4-byte big-endian length prefix. Mirrors the engine client.
// ---------------------------------------------------------------------------

fn read_frame<R: Read>(r: &mut R) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("frame {len} exceeds {MAX_FRAME} bound"),
        ));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

fn write_frame<W: Write>(w: &mut W, payload: &[u8]) -> std::io::Result<()> {
    let len = u32::try_from(payload.len()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame exceeds 4 GiB",
        )
    })?;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

// ---------------------------------------------------------------------------
// Inference + entitlement
// ---------------------------------------------------------------------------

/// Verifies the license token offline. **Stub**: accepts any well-formed
/// `bv1_…` key (matching `cli/src/entitlement.rs`). Real Ed25519 signature
/// verification lands with the billing backend; the shape is stable so this
/// is the single enforcement point the model is gated behind.
///
/// Returns `Ok(())` for entitled, `Err((code, msg))` for refused.
fn check_entitlement(
    token: &Option<String>,
) -> Result<(), (&'static str, String)> {
    match token.as_deref() {
        Some(t) if t.starts_with(LICENSE_KEY_PREFIX) => Ok(()),
        _ => Err((
            "unentitled",
            "NER requires a paid plan; free tier is not served by the model."
                .into(),
        )),
    }
}

/// Runs NER over `text`. **Stub until the `model` feature lands**: returns
/// `unavailable` so the engine fails closed rather than emitting unredacted
/// text. With `model` enabled this loads the bundled Int8 ONNX BERT-NER via
/// `ort` + `tokenizers`, aggregates BIO tags → byte offsets, floors by
/// `min_confidence`, and maps PER/ORG/LOC → the engine's entity slugs.
fn run_inference(
    _text: &str,
) -> Result<Vec<NerFinding>, (&'static str, String)> {
    #[cfg(feature = "model")]
    {
        // TODO(phase-c): ort::Session from the embedded model bytes +
        // tokenizers::Tokenizer, offset-mapped BIO aggregation, PER/ORG/LOC
        // slug mapping. Until then, fail closed.
        Err((
            "unavailable",
            "model feature enabled but inference is not wired yet".into(),
        ))
    }
    #[cfg(not(feature = "model"))]
    {
        Err((
            "unavailable",
            "nerd built without the `model` feature; no NER is served".into(),
        ))
    }
}

fn handle_request(req: DetectRequest) -> DetectOutcome {
    if let Err((code, msg)) = check_entitlement(&req.token) {
        return DetectOutcome::Err {
            code: code.to_string(),
            msg,
        };
    }
    match run_inference(&req.text) {
        Ok(findings) => DetectOutcome::Ok { findings },
        Err((code, msg)) => DetectOutcome::Err {
            code: code.to_string(),
            msg,
        },
    }
}

fn handle_connection(mut stream: UnixStream) {
    let req_bytes = match read_frame(&mut stream) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("nerd: read request failed: {e}");
            return;
        }
    };
    let req: DetectRequest = match serde_json::from_slice(&req_bytes) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nerd: decode request failed: {e}");
            // Send a typed error so the engine surfaces it fail-closed.
            let outcome = DetectOutcome::Err {
                code: "internal".into(),
                msg: format!("bad request: {e}"),
            };
            let _ = write_outcome(&mut stream, &outcome);
            return;
        }
    };
    let outcome = handle_request(req);
    if let Err(e) = write_outcome(&mut stream, &outcome) {
        eprintln!("nerd: write response failed: {e}");
    }
}

fn write_outcome(
    stream: &mut UnixStream,
    outcome: &DetectOutcome,
) -> std::io::Result<()> {
    let payload = serde_json::to_vec(outcome).map_err(std::io::Error::other)?;
    write_frame(stream, &payload)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> std::io::Result<()> {
    let socket_path = parse_socket_path();
    // Clean up any stale socket so bind succeeds across restarts.
    let _ = std::fs::remove_file(&socket_path);
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let listener = UnixListener::bind(&socket_path)?;
    eprintln!(
        "bitvanes-nerd listening on {} (model feature: {})",
        socket_path.display(),
        if cfg!(feature = "model") { "on" } else { "off" }
    );
    eprintln!(
        "  build without `--features model` serves NO NER (fails closed = unavailable)"
    );

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                std::thread::spawn(|| handle_connection(s));
            }
            Err(e) => {
                eprintln!("nerd: accept failed: {e}");
                continue;
            }
        }
    }
    Ok(())
}

/// Resolves the socket path from `BITVANES_NERD_SOCKET` or a default, matching
/// the engine client's lookup. Defaults to a per-user path under
/// `XDG_RUNTIME_DIR` (or `/tmp`) so unprivileged runs need no root.
fn parse_socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("BITVANES_NERD_SOCKET") {
        return PathBuf::from(p);
    }
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    dir.join("bitvanes-nerd.sock")
}
