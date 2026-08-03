//! PII detection: deterministic regex/Luhn/ABA engines, confidence scoring,
//! and the bidirectional [`OffsetMap`] for projecting redacted positions back
//! onto the original document.
//!
//! ## Layout
//!
//! - [`detect`] — the Tier-1 engine: built-in patterns (email, SSN, phone,
//!   credit card, routing number, API keys, JWT, street address), contextual
//!   anchor boosting, weighted-additive confidence, overlap resolution, and
//!   the [`Scrubber`] / [`scrub_document`] entry points.
//! - [`model`] — the Tier-2 plug-in trait [`PiiDetector`] for a local NER
//!   model (names, addresses, custom entities). Gated by `pii-model`.

pub mod detect;
pub mod model;

pub use detect::{OffsetMap, PiiFinding, Scrubber, scrub_document, scrub_text};
pub use model::{ModelDetector, PiiDetector};
