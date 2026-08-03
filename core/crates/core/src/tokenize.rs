//! Token counting and token-boundary splitting for the chunker.
//!
//! # Heuristic estimator (no BPE dependency)
//!
//! Token counts are approximated at **≈ 4 characters per token** — the
//! documented `OpenAI` rule of thumb for GPT-4 / GPT-4o English + code text.
//! This removes the heavy `tiktoken-rs` dependency (which embedded
//! multi-MB vocab files via `include_str!`) while keeping the chunker's
//! `max_tokens` budgeting behaviorally identical.
//!
//! The estimate is intentionally **conservative for chunk sizing**, not a
//! substitute for exact BPE counts: it exists so the chunker can pack spans
//! to a roughly-even size without pulling in a tokenizer runtime. If exact
//! invoice-grade token counts are needed, re-count the output with the
//! tokenizer of your downstream model.
//!
//! `TokenizerKind` is retained on [`ChunkConfig`](crate::schema::ChunkConfig)
//! for wire-format compatibility with existing `Bitvanes.toml` / profile JSON,
//! but every variant now resolves to the same heuristic.
//!
//! # Zero-telemetry
//!
//! There is no tokenizer model, no vocab file, and no network code. The
//! estimator is pure arithmetic over the input string.

use crate::error::Result;
use crate::schema::TokenizerKind;

/// Approximate characters per token (`OpenAI`'s documented average for
/// English + code text under GPT-4 / GPT-4o BPE).
const CHARS_PER_TOKEN: usize = 4;

/// A token counter / boundary splitter. Stateless beyond the (now
/// informational) [`TokenizerKind`].
///
/// Construct with [`Tokenizer::new`]; use [`Tokenizer::count`] for sizing and
/// [`Tokenizer::split_at_token_boundary`] / [`Tokenizer::suffix_tokens`] for
/// chunk-boundary math.
pub struct Tokenizer {
    kind: TokenizerKind,
}

impl std::fmt::Debug for Tokenizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokenizer")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl Tokenizer {
    /// Returns a tokenizer handle. `kind` is retained for configuration /
    /// serialization compatibility but does not change the estimate — all
    /// variants use the chars-per-token heuristic.
    ///
    /// # Errors
    ///
    /// Currently always returns `Ok`. The `Result` is retained so a future
    /// exact-tokenizer backend can fail without a breaking signature change.
    pub fn new(kind: TokenizerKind) -> Result<Self> {
        Ok(Self { kind })
    }

    /// Returns the configured [`TokenizerKind`] (informational only).
    #[must_use]
    pub const fn kind(&self) -> TokenizerKind {
        self.kind
    }

    /// Estimated token count of `text` (≈ chars ÷ 4, ceiling).
    #[must_use]
    pub fn count(&self, text: &str) -> usize {
        count_tokens(text)
    }

    /// Splits `text` at a character boundary that yields at most `max_tokens`
    /// tokens. Returns `(byte_offset, token_count)` where:
    ///
    /// - `byte_offset` is the byte position in `text` at which to cut
    ///   (always a UTF-8 character boundary, never 0 for non-empty input).
    /// - `token_count` is the estimated token count of the prefix
    ///   `[..byte_offset]` (≤ `max_tokens`).
    ///
    /// If the entire text estimates to `<= max_tokens` tokens, returns
    /// `(text.len(), token_count)`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::BitVanesError::InvalidInput`] if a non-empty
    /// input would produce a zero-length split (structurally impossible for
    /// the heuristic, but retained for API safety).
    pub fn split_at_token_boundary(&self, text: &str, max_tokens: usize) -> Result<(usize, usize)> {
        let total = count_tokens(text);
        if total <= max_tokens {
            return Ok((text.len(), total));
        }
        let char_budget = max_tokens.saturating_mul(CHARS_PER_TOKEN);
        let offset = nth_char_offset(text, char_budget);
        if offset == 0 {
            // No progress possible — force at least one character so the
            // chunker never loops on an oversized single span.
            let forced = nth_char_offset(text, 1);
            return Ok((forced, count_tokens(&text[..forced])));
        }
        let actual = count_tokens(&text[..offset]);
        Ok((offset, actual))
    }

    /// Returns the `(byte_offset, token_count)` of a suffix of `text` covering
    /// at most `n_tokens` tokens — the symmetric companion to
    /// [`split_at_token_boundary`], used to derive an overlap tail.
    ///
    /// - `byte_offset` is the start of the suffix (a UTF-8 character
    ///   boundary; `0` when the whole text is within `n_tokens`).
    /// - `token_count` is the estimated token count of `text[byte_offset..]`.
    ///
    /// # Errors
    ///
    /// Currently always returns `Ok`; the `Result` is retained for API
    /// compatibility with [`split_at_token_boundary`].
    pub fn suffix_tokens(&self, text: &str, n_tokens: usize) -> Result<(usize, usize)> {
        let total = count_tokens(text);
        if total <= n_tokens {
            return Ok((0, total));
        }
        let total_chars = text.chars().count();
        let suffix_chars = n_tokens.saturating_mul(CHARS_PER_TOKEN);
        let skip_chars = total_chars.saturating_sub(suffix_chars);
        let offset = nth_char_offset(text, skip_chars);
        let actual = count_tokens(&text[offset..]);
        Ok((offset, actual))
    }
}

/// Estimated token count: ceiling of `chars ÷ CHARS_PER_TOKEN`. Empty → 0.
fn count_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(CHARS_PER_TOKEN)
}

/// Returns the byte offset of the start of the `n`-th (0-indexed) character,
/// clamped to `text.len()`. Always a valid UTF-8 boundary.
fn nth_char_offset(text: &str, n: usize) -> usize {
    text.char_indices().nth(n).map_or(text.len(), |(b, _)| b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok() -> Tokenizer {
        Tokenizer::new(TokenizerKind::Cl100kBase).expect("tokenizer constructs")
    }

    #[test]
    fn token_count_is_nonzero_for_nonempty_text() {
        let t = tok();
        assert!(t.count("hello world") > 0);
    }

    #[test]
    fn token_count_grows_with_text() {
        let t = tok();
        let short = t.count("hello");
        let long = t.count("hello world this is a longer sentence");
        assert!(long > short, "{long} should be > {short}");
    }

    #[test]
    fn count_estimates_via_chars_per_token() {
        // Heuristic: ceil(chars / 4). "hello" = 5 chars → 2 tokens.
        let t = tok();
        assert_eq!(t.count(""), 0);
        assert_eq!(t.count("hello"), 2); // ceil(5/4)
        assert_eq!(t.count("hell"), 1); // 4 chars → 1
        assert_eq!(t.count("hello world"), 3); // 11 chars → 3
    }

    #[test]
    fn split_at_boundary_returns_full_text_when_under_limit() {
        let t = tok();
        let text = "hello";
        let (offset, count) = t.split_at_token_boundary(text, 100).unwrap();
        assert_eq!(offset, text.len());
        assert_eq!(count, t.count(text));
    }

    #[test]
    fn split_at_boundary_caps_at_max_tokens() {
        let t = tok();
        let text = "hello world this is a test of the tokenizer splitting";
        let (offset, count) = t.split_at_token_boundary(text, 3).unwrap();
        assert!(count <= 3, "token count {count} should be <= 3");
        assert!(offset <= text.len());
        assert!(offset > 0, "offset should be nonzero");
        // The prefix must be valid UTF-8 and non-empty.
        assert!(!text[..offset].is_empty(), "prefix should not be empty");
    }

    #[test]
    fn split_at_boundary_snaps_to_char_boundary() {
        let t = tok();
        // Use a multi-byte char to verify char-boundary snapping.
        let text = "héllo wörld тест тест";
        let (offset, _) = t.split_at_token_boundary(text, 2).unwrap();
        assert!(
            text.is_char_boundary(offset),
            "offset must be a char boundary"
        );
    }

    #[test]
    fn all_tokenizer_variants_load_and_count() {
        // Every variant must construct and count; all resolve to the same
        // heuristic, but the wire-format compatibility is what we guard here.
        for kind in [
            TokenizerKind::Cl100kBase,
            TokenizerKind::O200kBase,
            TokenizerKind::R50kBase,
            TokenizerKind::P50kBase,
            TokenizerKind::P50kEdit,
            TokenizerKind::O200kHarmony,
        ] {
            let t = Tokenizer::new(kind)
                .unwrap_or_else(|e| panic!("tokenizer {kind:?} should construct: {e}"));
            assert!(
                t.count("hello world") > 0,
                "tokenizer {kind:?} returned zero tokens for non-empty text"
            );
            assert_eq!(t.kind(), kind);
        }
    }

    #[test]
    fn token_count_of_empty_text_is_zero() {
        let t = tok();
        assert_eq!(t.count(""), 0);
    }

    #[test]
    fn suffix_tokens_returns_full_text_when_under_limit() {
        let t = tok();
        let text = "hello";
        let (offset, count) = t.suffix_tokens(text, 100).unwrap();
        assert_eq!(offset, 0);
        assert_eq!(count, t.count(text));
    }

    #[test]
    fn suffix_tokens_snaps_to_char_boundary_and_respects_cap() {
        let t = tok();
        let text = "héllo wörld тест тест more tokens here";
        let n = 3;
        let (offset, count) = t.suffix_tokens(text, n).unwrap();
        assert!(text.is_char_boundary(offset), "must be a char boundary");
        assert!(count <= n, "suffix token count {count} should be <= {n}");
        assert!(offset > 0, "suffix should not start at 0");
        assert_eq!(count, t.count(&text[offset..]));
    }
}
