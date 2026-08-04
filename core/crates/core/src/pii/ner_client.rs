//! Tier-2 NER client: the IPC client + wire contract that talks to the
//! `bitvanes-nerd` sidecar over a local socket. The engine itself stays
//! ML-free — this module is pure IPC (length-prefixed JSON frames), and the
//! model + ONNX Runtime live entirely in the sidecar.
//!
//! # Transport
//!
//! [`LocalTransport`] is a **synchronous** round-trip trait, matching the
//! synchronous [`Scrubber::scrub_with_detectors`](crate::pii::Scrubber)
//! pipeline (which is already doing CPU-bound regex work inline). Each
//! [`LocalTransport::round_trip`] opens a fresh connection — thread-safe by
//! construction (the `Arc<RemoteNerClient>` is cloned freely across a rayon
//! batch with no shared socket state) and cheap relative to NER inference.
//!
//! - Unix: [`unix::UnixSocketTransport`] over a `AF_UNIX` stream socket.
//! - Windows: a named-pipe transport (todo — needs `windows-sys` + a Windows
//!   CI run; the trait seam means it slots in without touching the client).
//!
//! # Wire contract
//!
//! Length-prefixed (4-byte big-endian) JSON:
//!
//! ```text
//! Request  -> DetectRequest { text, token }
//! Response <- DetectOutcome { Ok { findings }, Err { code, msg } }
//! ```
//!
//! `token` is the `bv1_…` license key. The sidecar verifies it **offline**
//! and refuses free-tier use with `code = "unentitled"` — enforcement lives
//! where the model actually runs, so a client that bypasses the gate gets
//! nothing. The CLI additionally does not attach this client on the free
//! tier (fail-fast UX); the sidecar check is defense-in-depth.
//!
//! # Fail-closed
//!
//! Any transport or decode error (sidecar down, malformed frame, sidecar
//! `Err`) surfaces as [`BitVanesError::Inference`], which
//! `scrub_with_detectors` propagates — the pipeline refuses to emit text
//! the detector would have scrubbed.
//!
//! Gated by the `ner-client` feature.

use std::fmt;
use std::io::{Read, Write};

use serde::{Deserialize, Serialize};

use crate::error::{BitVanesError, Result};
use crate::pii::detect::PiiFinding;
use crate::pii::model::PiiDetector;

/// Upper bound on a single frame (request or response) so a buggy or
/// hostile peer cannot force an oversized allocation. 256 MiB is generous
/// for a whole-document request + a findings list; bounded-memory invariant
/// is preserved regardless of input.
const MAX_FRAME: usize = 256 * 1024 * 1024;

// ===========================================================================
// Wire contract
// ===========================================================================

/// Request body sent to the sidecar.
#[derive(Debug, Serialize, Deserialize)]
pub struct DetectRequest<'a> {
    /// The document text to scan. Offsets in the response are byte offsets
    /// into this exact string.
    pub text: &'a str,
    /// The `bv1_…` license key, when the caller is entitled. The sidecar
    /// verifies it offline; `None` ⇒ free-tier ⇒ sidecar refuses with
    /// `code = "unentitled"`.
    pub token: Option<&'a str>,
}

/// A single NER finding in the wire response. Offsets are **byte** offsets
/// into the request `text`, matching [`PiiFinding::offset_start`] / `_end`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NerFinding {
    pub entity: String,
    pub start: u32,
    pub end: u32,
    pub confidence: f32,
}

/// The sidecar's response: either the findings list or a typed refusal.
///
/// `unentitled` — license missing/invalid/free-tier (model not served).
/// `unavailable` — transient: model not loaded, OOM, etc.
/// `internal` — unexpected sidecar fault.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DetectOutcome {
    Ok { findings: Vec<NerFinding> },
    Err { code: String, msg: String },
}

// ===========================================================================
// Transport trait
// ===========================================================================

/// A synchronous, connection-per-call local transport (Unix domain socket
/// or Windows named pipe). Object-safe so [`RemoteNerClient`] can hold a
/// `Box<dyn LocalTransport>` and so the concrete transport can be swapped
/// per-platform / faked in tests.
pub trait LocalTransport: Send + Sync + fmt::Debug {
    /// Send `request_payload` and return the `response_payload`. Each call
    /// opens a fresh connection (see module docs for why).
    ///
    /// # Errors
    ///
    /// Implementations MUST map any I/O or framing failure to
    /// [`BitVanesError::Inference`] so the pipeline fails closed.
    fn round_trip(&self, request_payload: &[u8]) -> Result<Vec<u8>>;
}

// ===========================================================================
// Length-prefixed framing (4-byte big-endian)
// ===========================================================================

fn io_err(context: &str, e: &std::io::Error) -> BitVanesError {
    BitVanesError::Inference(format!("{context}: {e}"))
}

fn write_frame<W: Write>(w: &mut W, payload: &[u8]) -> Result<()> {
    let len = u32::try_from(payload.len())
        .map_err(|_| BitVanesError::Inference("nerd request exceeds 4 GiB frame limit".into()))?;
    w.write_all(&len.to_be_bytes())
        .map_err(|e| io_err("nerd write length", &e))?;
    w.write_all(payload)
        .map_err(|e| io_err("nerd write payload", &e))?;
    w.flush().map_err(|e| io_err("nerd flush", &e))?;
    Ok(())
}

fn read_frame<R: Read>(r: &mut R) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)
        .map_err(|e| io_err("nerd read length (sidecar down?)", &e))?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        return Err(BitVanesError::Inference(format!(
            "nerd response frame {len} exceeds {MAX_FRAME} bound"
        )));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)
        .map_err(|e| io_err("nerd read payload", &e))?;
    Ok(buf)
}

// ===========================================================================
// RemoteNerClient — the PiiDetector impl over the transport
// ===========================================================================

/// Tier-2 PII detector backed by the `bitvanes-nerd` sidecar. Holds a
/// transport and the caller's license token; implements [`PiiDetector`] so
/// it plugs straight into
/// [`Scrubber::with_detectors`](crate::pii::Scrubber::with_detectors).
///
/// Clone-cheap behind `Arc` for the rayon batch path.
#[derive(Debug)]
pub struct RemoteNerClient {
    transport: Box<dyn LocalTransport>,
    token: Option<Box<str>>,
}

impl RemoteNerClient {
    /// Creates a client over `transport`. `token` is the `bv1_…` license key
    /// (`None` ⇒ the sidecar will refuse as `unentitled`).
    #[must_use]
    pub fn new(transport: Box<dyn LocalTransport>, token: Option<String>) -> Self {
        Self {
            transport,
            token: token.map(Box::from),
        }
    }
}

impl PiiDetector for RemoteNerClient {
    fn detect(&self, text: &str, findings: &mut Vec<PiiFinding>) -> Result<()> {
        let req = DetectRequest {
            text,
            token: self.token.as_deref(),
        };
        let req_bytes = serde_json::to_vec(&req)
            .map_err(|e| BitVanesError::Inference(format!("nerd encode request: {e}")))?;
        let resp_bytes = self.transport.round_trip(&req_bytes)?;
        let outcome: DetectOutcome = serde_json::from_slice(&resp_bytes)
            .map_err(|e| BitVanesError::Inference(format!("nerd decode response: {e}")))?;

        match outcome {
            DetectOutcome::Ok { findings: ner } => {
                for f in ner {
                    findings.push(PiiFinding {
                        entity: f.entity,
                        offset_start: f.start,
                        offset_end: f.end,
                        confidence: f.confidence,
                        anchors_hit: Vec::new(),
                    });
                }
                Ok(())
            }
            DetectOutcome::Err { code, msg } => Err(BitVanesError::Inference(format!(
                "nerd refused: {code}: {msg}"
            ))),
        }
    }
}

// ===========================================================================
// Unix domain socket transport
// ===========================================================================

#[cfg(unix)]
pub mod unix {
    use super::{LocalTransport, read_frame, write_frame};
    use crate::error::Result;
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;

    /// `AF_UNIX` stream transport. One fresh connection per round-trip.
    #[derive(Debug, Clone)]
    pub struct UnixSocketTransport {
        path: PathBuf,
    }

    impl UnixSocketTransport {
        /// Creates a transport that connects to the sidecar listening at
        /// `path` (typically `${BITVANES_NERD_SOCKET:-/run/bitvanes/nerd.sock}`
        /// or a per-user path under `XDG_RUNTIME_DIR`).
        #[must_use]
        pub fn new(path: impl Into<PathBuf>) -> Self {
            Self { path: path.into() }
        }

        /// The socket path this transport connects to.
        #[must_use]
        pub fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl LocalTransport for UnixSocketTransport {
        fn round_trip(&self, request_payload: &[u8]) -> Result<Vec<u8>> {
            let mut stream = UnixStream::connect(&self.path).map_err(|e| {
                super::io_err(
                    &format!(
                        "connect nerd at {} (is `bitvanes-nerd` running?)",
                        self.path.display()
                    ),
                    &e,
                )
            })?;
            write_frame(&mut stream, request_payload)?;
            read_frame(&mut stream)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[cfg(unix)]
    use crate::pii::Scrubber;
    #[cfg(unix)]
    use crate::schema::{BuiltInPattern, ScrubProfile};

    // ----- framing -----

    #[test]
    fn framing_round_trips_a_payload() {
        let mut buf = Vec::new();
        let payload = b"{\"hello\":\"nerd\"}";
        write_frame(&mut buf, payload).expect("write");
        // 4-byte BE length + payload
        assert_eq!(
            &buf[..4],
            &u32::try_from(payload.len()).unwrap().to_be_bytes()
        );
        assert_eq!(&buf[4..], payload);

        let mut cursor = std::io::Cursor::new(buf);
        let out = read_frame(&mut cursor).expect("read");
        assert_eq!(out, payload);
    }

    #[test]
    fn read_frame_rejects_oversized_length() {
        let mut buf = (u32::try_from(MAX_FRAME).unwrap() + 1)
            .to_be_bytes()
            .to_vec();
        buf.resize(buf.len() + 4, 0); // padding so read_exact isn't short
        let mut cursor = std::io::Cursor::new(buf);
        let err = read_frame(&mut cursor).expect_err("must reject");
        assert!(matches!(err, BitVanesError::Inference(_)));
    }

    // ----- contract ser/de -----

    #[test]
    fn detect_outcome_round_trips() {
        let ok = DetectOutcome::Ok {
            findings: vec![NerFinding {
                entity: "person_name".into(),
                start: 4,
                end: 9,
                confidence: 0.97,
            }],
        };
        let s = serde_json::to_vec(&ok).unwrap();
        let back: DetectOutcome = serde_json::from_slice(&s).unwrap();
        match back {
            DetectOutcome::Ok { findings } => {
                assert_eq!(findings[0].entity, "person_name");
                assert_eq!(findings[0].start, 4);
            }
            DetectOutcome::Err { .. } => panic!("expected Ok"),
        }

        let err = DetectOutcome::Err {
            code: "unentitled".into(),
            msg: "free tier; NER requires a paid plan".into(),
        };
        let s = serde_json::to_vec(&err).unwrap();
        let back: DetectOutcome = serde_json::from_slice(&s).unwrap();
        match back {
            DetectOutcome::Err { code, .. } => assert_eq!(code, "unentitled"),
            DetectOutcome::Ok { .. } => panic!("expected Err"),
        }
    }

    // ----- in-memory transport (logic test, no socket) -----

    /// A transport that returns a canned frame regardless of input.
    #[derive(Debug)]
    struct CannedTransport {
        resp: Vec<u8>,
    }
    impl LocalTransport for CannedTransport {
        fn round_trip(&self, _request_payload: &[u8]) -> Result<Vec<u8>> {
            Ok(self.resp.clone())
        }
    }

    #[derive(Debug)]
    struct FailingTransport;
    impl LocalTransport for FailingTransport {
        fn round_trip(&self, _request_payload: &[u8]) -> Result<Vec<u8>> {
            Err(BitVanesError::Inference("transport down".into()))
        }
    }

    #[test]
    fn remote_client_maps_ok_findings_into_pii_findings() {
        let outcome = DetectOutcome::Ok {
            findings: vec![NerFinding {
                entity: "person_name".into(),
                start: 0,
                end: 5,
                confidence: 0.9,
            }],
        };
        // `round_trip` returns the *payload* (the transport owns framing),
        // so the canned response is raw JSON — no write_frame wrapper.
        let resp = serde_json::to_vec(&outcome).unwrap();

        let client = RemoteNerClient::new(Box::new(CannedTransport { resp }), None);
        let mut findings = Vec::new();
        client.detect("hello", &mut findings).expect("ok");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].entity, "person_name");
        assert_eq!((findings[0].offset_start, findings[0].offset_end), (0, 5));
    }

    #[test]
    fn remote_client_surfaces_sidecar_refusal_as_inference_error() {
        // Sidecar refuses (free-tier / unentitled) → client MUST surface it as
        // a fail-closed Inference error, never silently emit nothing.
        let outcome = DetectOutcome::Err {
            code: "unentitled".into(),
            msg: "free tier".into(),
        };
        let resp = serde_json::to_vec(&outcome).unwrap();

        let client = RemoteNerClient::new(Box::new(CannedTransport { resp }), None);
        let mut findings = Vec::new();
        let err = client
            .detect("hello", &mut findings)
            .expect_err("must fail");
        assert!(matches!(err, BitVanesError::Inference(_)));
        assert!(err.to_string().contains("unentitled"));
        assert!(findings.is_empty(), "no findings on refusal");
    }

    #[test]
    fn remote_client_fails_closed_on_transport_error() {
        let client = RemoteNerClient::new(Box::new(FailingTransport), None);
        let mut findings = Vec::new();
        let err = client
            .detect("hello", &mut findings)
            .expect_err("must fail");
        assert!(matches!(err, BitVanesError::Inference(_)));
    }

    // ----- end-to-end over a real Unix domain socket -----

    #[cfg(unix)]
    #[test]
    fn unix_transport_round_trips_against_a_mock_sidecar() {
        use std::os::unix::net::UnixListener;
        use std::thread;

        let path = unique_socket_path();

        // Bind the mock sidecar.
        let listener = UnixListener::bind(&path).expect("bind");
        let text_for_server = Arc::new(String::from(
            "Email alice@example.com or call Alice Smith today.",
        ));
        let text_server = Arc::clone(&text_for_server);
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let req = read_frame(&mut stream).expect("read req");
            let req: DetectRequest = serde_json::from_slice(&req).expect("decode req");
            assert_eq!(req.text, text_server.as_str());
            assert_eq!(req.token, Some("bv1_test_token"));

            // Pretend the model found "Alice Smith" at its byte offset.
            let name_start =
                u32::try_from(req.text.find("Alice Smith").expect("server sees the name")).unwrap();
            let outcome = DetectOutcome::Ok {
                findings: vec![NerFinding {
                    entity: "person_name".into(),
                    start: name_start,
                    end: name_start + u32::try_from("Alice Smith".len()).unwrap(),
                    confidence: 0.96,
                }],
            };
            let mut resp = Vec::new();
            write_frame(&mut resp, &serde_json::to_vec(&outcome).unwrap()).unwrap();
            stream.write_all(&resp).unwrap();
        });

        let scrubber = Scrubber::from_profile(&ScrubProfile {
            patterns: vec![BuiltInPattern::Email],
            ..ScrubProfile::default()
        })
        .expect("patterns compile")
        .with_detectors(vec![Arc::new(RemoteNerClient::new(
            Box::new(unix::UnixSocketTransport::new(&path)),
            Some("bv1_test_token".to_string()),
        ))]);

        let (out, _map, findings) = scrubber
            .scrub_with_detectors(&text_for_server)
            .expect("detect ok");

        handle.join().unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(out.contains("[EMAIL]"), "tier-1 email: {out}");
        assert!(out.contains("[PERSON]"), "tier-2 name: {out}");
        assert!(!out.contains("Alice"), "name gone: {out}");
        let entities: Vec<&str> = findings.iter().map(|f| f.entity.as_str()).collect();
        assert!(entities.contains(&"person_name"));
        assert!(entities.contains(&"email"));
    }

    #[cfg(unix)]
    #[test]
    fn unix_transport_fails_closed_when_sidecar_absent() {
        // Nothing is listening here.
        let path = unique_socket_path();

        let scrubber = Scrubber::from_profile(&ScrubProfile {
            patterns: vec![BuiltInPattern::Email],
            ..ScrubProfile::default()
        })
        .expect("patterns compile")
        .with_detectors(vec![Arc::new(RemoteNerClient::new(
            Box::new(unix::UnixSocketTransport::new(&path)),
            Some("bv1_test_token".to_string()),
        ))]);

        let err = scrubber
            .scrub_with_detectors("alice@example.com")
            .expect_err("must fail closed when sidecar is down");
        assert!(matches!(err, BitVanesError::Inference(_)));
        let _ = std::fs::remove_file(&path);
    }

    /// Monotonic unique socket path under `/tmp` — no `tempfile` dep needed.
    #[cfg(unix)]
    fn unique_socket_path() -> std::path::PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        std::path::PathBuf::from(format!(
            "/tmp/bitvanes-ner-test-{}-{n}.sock",
            std::process::id()
        ))
    }
}
