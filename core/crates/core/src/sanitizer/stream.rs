//! Bounded-memory async stream sanitizer.
//!
//! Reads an unbounded byte stream from any `tokio::io::AsyncRead`, redacts PII
//! in a **rolling window**, and writes sanitized output to an `AsyncWrite` —
//! without ever buffering the whole input. Memory use is `O(max_match_len)`.
//!
//! # Boundary correctness (the core invariant)
//!
//! A PII token split across two read chunks must still be redacted. We hold
//! back the trailing `max_match_len` bytes of the buffer (plus any finding
//! that overlaps that tail) until more data arrives, so a match can never be
//! prematurely emitted in pieces. `max_match_len` MUST be at least as large as
//! the longest possible match of any enabled pattern; the default is 1024
//! bytes (covers email, SSN, phone, credit card, routing, street address, and
//! typical JWTs). Raise it if you enable patterns whose tokens can exceed it.
//!
//! # UTF-8 safety
//!
//! Read chunks can split a multi-byte UTF-8 sequence. We only ever operate on
//! the longest valid-UTF-8 prefix of the buffer and hold the trailing
//! incomplete bytes back until the next read.
//!
//! Applicable to text/JSON/CSV streams. Binary container formats (PDF, DOCX,
//! ...) are not streamable as plain text — they go through the document path.
//!
//! Enabled behind the `stream` cargo feature.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{BitVanesError, Result};
use crate::pii::{PiiFinding, Scrubber};
use crate::sanitizer::policy::{RedactionPolicy, sanitize_text};

/// Default rolling-window size in bytes. Covers every built-in pattern's
/// realistic maximum length.
pub const DEFAULT_MAX_MATCH_LEN: usize = 1024;

/// Bounded-memory async PII sanitizer over `tokio::io`.
///
/// Construct with a [`Scrubber`] (compiled once) and a [`RedactionPolicy`],
/// then drive it with [`StreamSanitizer::sanitize_reader`]. Findings are
/// returned with offsets relative to the **original** stream.
pub struct StreamSanitizer {
    scrubber: Scrubber,
    policy: RedactionPolicy,
    max_match_len: usize,
    buf: Vec<u8>,
    /// Number of original-stream bytes already emitted.
    global_offset: u64,
    /// Findings reported so far, with absolute stream offsets.
    findings: Vec<PiiFinding>,
}

impl StreamSanitizer {
    /// Creates a new sanitizer with the default match window
    /// ([`DEFAULT_MAX_MATCH_LEN`]).
    #[must_use]
    pub fn new(scrubber: Scrubber, policy: RedactionPolicy) -> Self {
        Self::with_max_match_len(scrubber, policy, DEFAULT_MAX_MATCH_LEN)
    }

    /// Creates a new sanitizer with an explicit match window. `max_match_len`
    /// must be greater than zero and should be ≥ the longest possible match of
    /// any enabled pattern (see the module-level invariant note).
    #[must_use]
    pub fn with_max_match_len(
        scrubber: Scrubber,
        policy: RedactionPolicy,
        max_match_len: usize,
    ) -> Self {
        Self {
            scrubber,
            policy,
            max_match_len: max_match_len.max(1),
            buf: Vec::new(),
            global_offset: 0,
            findings: Vec::new(),
        }
    }

    /// Returns the findings accumulated so far (absolute stream offsets).
    #[must_use]
    pub fn findings(&self) -> &[PiiFinding] {
        &self.findings
    }

    /// Reads `reader` to EOF, writing redacted output to `writer`. Returns the
    /// list of findings (absolute stream offsets).
    ///
    /// # Errors
    ///
    /// Propagates [`BitVanesError`] from the scrubber/sanitizer or IO from the
    /// reader/writer.
    pub async fn sanitize_reader<R, W>(
        &mut self,
        mut reader: R,
        writer: &mut W,
    ) -> Result<Vec<PiiFinding>>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let mut tmp = vec![0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut tmp).await.map_err(BitVanesError::from)?;
            if n == 0 {
                self.flush_eof(writer).await?;
                writer.flush().await.map_err(BitVanesError::from)?;
                return Ok(std::mem::take(&mut self.findings));
            }
            self.buf.extend_from_slice(&tmp[..n]);
            // Pump until no more progress can be made with the current buffer.
            while self.pump(writer).await? {}
        }
    }

    /// Processes the current buffer once: emits the safe prefix and retains
    /// the tail. Returns `Ok(true)` if progress was made, `Ok(false)` if more
    /// data is needed.
    async fn pump<W: AsyncWrite + Unpin>(&mut self, writer: &mut W) -> Result<bool> {
        // Reduce to the longest valid-UTF-8 prefix (hold the incomplete tail).
        let valid_len = match std::str::from_utf8(&self.buf) {
            Ok(_) => self.buf.len(),
            Err(e) => e.valid_up_to(),
        };
        if valid_len <= self.max_match_len {
            return Ok(false); // need more bytes before a safe emit point exists
        }
        let safe_end = valid_len - self.max_match_len;

        let whole = std::str::from_utf8(&self.buf[..valid_len])
            .expect("valid UTF-8 prefix validated above");
        let (_engine_redacted, _offset_map, findings_rel) = self.scrubber.scrub(whole);

        // Hold back any finding that extends past the safe point: its start
        // becomes the new emit boundary so the (possibly incomplete) match is
        // retried once more data arrives.
        let mut emit_end = safe_end;
        for f in &findings_rel {
            if (f.offset_end as usize) > safe_end {
                emit_end = emit_end.min(f.offset_start as usize);
            }
        }
        if emit_end == 0 {
            return Ok(false); // a finding starts at 0 and crosses the tail; need more data
        }

        // Findings fully inside the emit window (relative to `whole`).
        let window = &whole[..emit_end];
        let emitted_rel: Vec<PiiFinding> = findings_rel
            .iter()
            .filter(|f| (f.offset_end as usize) <= emit_end)
            .cloned()
            .collect();

        // Build the policy output from the window + the relative findings.
        let out = sanitize_text(window, &emitted_rel, &self.policy)?;
        writer
            .write_all(out.as_bytes())
            .await
            .map_err(BitVanesError::from)?;

        // Record findings with absolute stream offsets.
        let base = u32::try_from(self.global_offset).unwrap_or(u32::MAX);
        for f in &emitted_rel {
            self.findings.push(PiiFinding {
                entity: f.entity.clone(),
                offset_start: f.offset_start + base,
                offset_end: f.offset_end + base,
                confidence: f.confidence,
                anchors_hit: f.anchors_hit.clone(),
            });
        }

        // Drop the emitted prefix; retain [emit_end..] (held findings + the
        // max_match_len tail zone + any incomplete UTF-8 bytes).
        self.buf.drain(0..emit_end);
        self.global_offset += emit_end as u64;
        Ok(true)
    }

    /// Emits everything remaining in the buffer (no holding back) and reports
    /// the final findings.
    async fn flush_eof<W: AsyncWrite + Unpin>(&mut self, writer: &mut W) -> Result<()> {
        let valid_len = match std::str::from_utf8(&self.buf) {
            Ok(_) => self.buf.len(),
            Err(e) => e.valid_up_to(),
        };
        if valid_len == 0 {
            // Only incomplete UTF-8 bytes remain; emit them verbatim (nothing
            // to redact in a partial code unit) so the stream length matches.
            writer
                .write_all(&self.buf)
                .await
                .map_err(BitVanesError::from)?;
            self.buf.clear();
            return Ok(());
        }
        let whole = std::str::from_utf8(&self.buf[..valid_len])
            .expect("valid UTF-8 prefix validated above");
        let (_redacted, _map, findings_rel) = self.scrubber.scrub(whole);
        let out = sanitize_text(whole, &findings_rel, &self.policy)?;
        writer
            .write_all(out.as_bytes())
            .await
            .map_err(BitVanesError::from)?;
        if valid_len < self.buf.len() {
            // Trailing incomplete UTF-8 bytes: emit verbatim.
            writer
                .write_all(&self.buf[valid_len..])
                .await
                .map_err(BitVanesError::from)?;
        }
        let base = u32::try_from(self.global_offset).unwrap_or(u32::MAX);
        for f in findings_rel {
            self.findings.push(PiiFinding {
                entity: f.entity,
                offset_start: f.offset_start + base,
                offset_end: f.offset_end + base,
                confidence: f.confidence,
                anchors_hit: f.anchors_hit,
            });
        }
        self.buf.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{BuiltInPattern, ScrubProfile};

    fn make_sanitizer(max_match_len: usize) -> StreamSanitizer {
        let scrubber = Scrubber::from_profile(&ScrubProfile {
            patterns: vec![BuiltInPattern::Email, BuiltInPattern::Ssn],
            ..ScrubProfile::default()
        })
        .expect("scrubber compiles");
        StreamSanitizer::with_max_match_len(scrubber, RedactionPolicy::default(), max_match_len)
    }

    #[tokio::test]
    async fn single_chunk_redacts_all_pii() {
        let mut s = make_sanitizer(64);
        let input = b"contact alice@example.com about ssn 123-45-6789 please.";
        let mut out = Vec::new();
        let findings = s.sanitize_reader(&input[..], &mut out).await.unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(!out.contains("alice@example.com"));
        assert!(!out.contains("123-45-6789"));
        assert!(out.contains("[REDACTED_EMAIL]"));
        assert!(out.contains("[REDACTED_SSN]"));
        assert_eq!(findings.len(), 2);
    }

    #[tokio::test]
    async fn pii_split_across_chunk_boundary_is_still_redacted() {
        // Window of 24 bytes; force 5-byte reads so the email straddles many
        // pump boundaries.
        let combined: Vec<u8> = b"mail alice@example.com now".to_vec();
        let chunked = ChunkedReader::new(combined.clone(), 5);
        let mut s = make_sanitizer(24);
        let mut out = Vec::new();
        let findings = s.sanitize_reader(chunked, &mut out).await.unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(
            !out.contains("alice@example.com"),
            "email must not survive: {out}"
        );
        assert!(
            out.contains("[REDACTED_EMAIL]"),
            "missing placeholder: {out}"
        );
        assert_eq!(findings.len(), 1);
        // Output text (ignoring the placeholder) still reconstructs the words.
        assert!(out.starts_with("mail "));
        assert!(out.ends_with(" now"));
    }

    #[tokio::test]
    async fn split_utf8_codepoint_is_held_intact() {
        let mut s = make_sanitizer(8);
        // "é" is 0xC3 0xA9. Split between the two bytes across 2-byte reads.
        let input: Vec<u8> = vec![b'x', 0xC3, 0xA9, b'y'];
        let chunked = ChunkedReader::new(input.clone(), 2);
        let mut out = Vec::new();
        s.sanitize_reader(chunked, &mut out).await.unwrap();
        assert_eq!(out, input, "codepoint must be reconstructed verbatim");
    }

    #[tokio::test]
    async fn empty_input_produces_empty_output() {
        let mut s = make_sanitizer(32);
        let empty: &[u8] = b"";
        let mut out = Vec::new();
        let findings = s.sanitize_reader(empty, &mut out).await.unwrap();
        assert!(out.is_empty());
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn mask_policy_is_applied_to_stream() {
        let scrubber = Scrubber::from_profile(&ScrubProfile {
            patterns: vec![BuiltInPattern::Ssn],
            ..ScrubProfile::default()
        })
        .unwrap();
        let mut s = StreamSanitizer::with_max_match_len(
            scrubber,
            RedactionPolicy::Mask { mask_char: '#' },
            32,
        );
        let input = b"ssn 123-45-6789 end";
        let mut out = Vec::new();
        s.sanitize_reader(&input[..], &mut out).await.unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("###########"), "expected 11 '#': {out}");
        assert!(!out.contains("123-45-6789"));
    }

    #[tokio::test]
    async fn large_input_does_not_accumulate_unbounded_buffer() {
        // With a 24-byte window, the retained buffer must stay bounded
        // (~window + chunk) regardless of total input size.
        let mut s = make_sanitizer(24);
        let chunked = ChunkedReader::new(vec![b'A'; 100_000], 1024);
        let mut out = Vec::new();
        s.sanitize_reader(chunked, &mut out).await.unwrap();
        // No PII → output equals input, and buffer was drained.
        assert_eq!(out.len(), 100_000);
        assert!(s.buf.is_empty());
    }

    /// A reader that yields at most `max` bytes per `read` call, to force
    /// stream chunking in tests.
    struct ChunkedReader {
        data: Vec<u8>,
        pos: usize,
        max: usize,
    }

    impl ChunkedReader {
        fn new(data: Vec<u8>, max: usize) -> Self {
            Self { data, pos: 0, max }
        }
    }

    impl AsyncRead for ChunkedReader {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            if self.pos >= self.data.len() {
                return std::task::Poll::Ready(Ok(()));
            }
            let cap = buf.capacity().min(self.max).max(1);
            let remaining = &self.data[self.pos..];
            let n = remaining.len().min(cap);
            buf.put_slice(&remaining[..n]);
            self.pos += n;
            std::task::Poll::Ready(Ok(()))
        }
    }

    proptest::proptest! {
        /// Invariant #3: an SSN split across an arbitrary byte boundary must
        /// still be redacted. Drives the sanitizer one byte per read so the
        /// token straddles many pump boundaries, with a small match window.
        #[test]
        fn ssn_redacted_regardless_of_stream_chunk_boundary(prefix in "[a-zA-Z0-9 ,.]{0,80}") {
            const SSN: &str = "123-45-6789";
            let input = format!("{prefix} contact {SSN} then more text here");
            let scrubber = Scrubber::from_profile(&ScrubProfile {
                patterns: vec![BuiltInPattern::Ssn],
                ..ScrubProfile::default()
            })
            .expect("scrubber compiles");

            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("rt");
            let out = rt.block_on(async move {
                // Window MUST be >= SSN length (11); use 64 for headroom.
                let mut s = StreamSanitizer::with_max_match_len(
                    scrubber,
                    RedactionPolicy::Placeholder,
                    64,
                );
                // 1 byte per read forces the SSN to span many chunk boundaries.
                let mut chunked = ChunkedReader::new(input.into_bytes(), 1);
                let mut out = Vec::new();
                s.sanitize_reader(&mut chunked, &mut out).await.unwrap();
                out
            });
            let out = String::from_utf8(out).unwrap();
            proptest::prop_assert!(
                !out.contains(SSN),
                "SSN leaked across chunk boundary; prefix={prefix:?} out={out}"
            );
            proptest::prop_assert!(
                out.contains("[REDACTED_SSN]"),
                "missing placeholder: {out}"
            );
        }
    }
}
