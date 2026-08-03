//! Entitlement / license-check hook.
//!
//! The BitVanes **engine** is local-first and zero-trust: it never makes
//! network calls. Commercial *entitlement* (is this customer allowed to run
//! this build, under which plan/seats?) is a deployment concern handled by a
//! separate cloud billing backend (Stripe + a license server) that is out of
//! scope for the engine itself.
//!
//! This module defines the *interface* the daemon calls before serving paid
//! features, plus a default [`EntitlementChecker`] impl that is permissive
//! (`OpenSource`). A future release ships a `CloudChecker` that validates a
//! signed license key against the billing backend — without changing the call
//! sites wired here.
//!
//! # Wire format (future, documented for the backend contract)
//!
//! A license key is `bv1_<base64 payload>.<sig>` where the payload encodes
//! `{ plan, seats, not_after }` and `sig` is an Ed25519 signature over the
//! payload. The local checker verifies the signature against an embedded
//! public key (offline, constant-time) and caches the result. No telemetry is
//! sent: validation is one-shot and local.

// This module is a deliberately-stable interface for the not-yet-implemented
// cloud billing backend. Several items (plan tiers, the license-key fields,
// the Invalid status) are part of that documented contract and are exercised
// by tests but not yet by the daemon's happy path — allow them until the
// `CloudChecker` lands.
#![allow(dead_code)]

use std::sync::Arc;

/// Service plan tiers mirrored from the billing backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Plan {
    /// Free / open-source build. No license key present.
    #[default]
    OpenSource,
    /// Paid tier (individual / solo).
    Pro,
    /// Paid tier (team / multi-seat).
    Team,
    /// Paid tier (enterprise / self-hosted).
    Enterprise,
}

impl Plan {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenSource => "open-source",
            Self::Pro => "pro",
            Self::Team => "team",
            Self::Enterprise => "enterprise",
        }
    }
}

/// The resolved entitlement for the running daemon. The daemon surfaces this
/// on startup and via `/health`; request handlers may call
/// [`EntitlementStatus::allow_paid_features`] to gate paid behavior.
#[derive(Debug, Clone)]
pub enum EntitlementStatus {
    /// No license key configured — the open-source floor.
    OpenSource,
    /// A license key was supplied and (eventually) validated.
    Licensed { plan: Plan, seats: u32 },
    /// A key was supplied but is structurally invalid or unsupported.
    /// `fail_open` governs whether the daemon serves anyway.
    Invalid { reason: String },
}

impl EntitlementStatus {
    /// Returns `true` if paid features may be served under this status.
    ///
    /// `OpenSource` and `Licensed` are allowed; `Invalid` is denied. When the
    /// cloud backend lands, a network/verification failure maps to `Invalid`
    /// and the daemon's configured fail-mode (`fail_open`) decides whether to
    /// keep serving.
    #[must_use]
    pub fn allow_paid_features(&self) -> bool {
        match self {
            Self::OpenSource | Self::Licensed { .. } => true,
            Self::Invalid { .. } => false,
        }
    }

    /// Short human-readable label for logs / `/health`.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::OpenSource => "open-source".to_string(),
            Self::Licensed { plan, seats } => format!("{} ({} seats)", plan.as_str(), seats),
            Self::Invalid { reason } => format!("invalid: {reason}"),
        }
    }
}

/// The hook the daemon calls to resolve entitlement. Implementations are
/// expected to be cheap and cached (the license key is verified once at
/// startup, not per request).
pub trait EntitlementChecker: Send + Sync {
    /// Resolves the current [`EntitlementStatus`].
    fn status(&self) -> EntitlementStatus;
}

/// Default permissive checker: no license key present → open-source floor.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenSourceChecker;

impl EntitlementChecker for OpenSourceChecker {
    fn status(&self) -> EntitlementStatus {
        EntitlementStatus::OpenSource
    }
}

/// A license key supplied via `--license-key` or `BITVANES_LICENSE_KEY`.
///
/// This is a **stub**: it recognizes the `bv1_` prefix and rejects clearly
/// malformed keys, but does NOT cryptographically validate them yet. A future
/// release will verify the Ed25519 signature locally (still no network). The
/// type exists now so config plumbing and the daemon banner are stable.
#[derive(Debug, Clone)]
pub struct LicenseKey {
    raw: String,
}

/// Expected license-key prefix (version 1 of the wire format).
pub const LICENSE_KEY_PREFIX: &str = "bv1_";

impl LicenseKey {
    /// Parses a raw key string into a [`LicenseKey`].
    ///
    /// # Errors
    ///
    /// Returns `Err` if the key is empty or does not start with `bv1_`.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err("license key is empty".to_string());
        }
        if !trimmed.starts_with(LICENSE_KEY_PREFIX) {
            return Err(format!(
                "license key must start with '{LICENSE_KEY_PREFIX}'"
            ));
        }
        Ok(Self {
            raw: trimmed.to_string(),
        })
    }

    /// The raw key string (including prefix).
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

/// A stub checker that holds a parsed [`LicenseKey`] and resolves to
/// `Licensed { plan: Pro, seats: 1 }` until the cloud signature-verification
/// backend is implemented. Replace with a `CloudChecker` (same trait) without
/// touching call sites.
#[derive(Debug, Clone)]
pub struct LicenseChecker {
    key: LicenseKey,
}

impl LicenseChecker {
    /// Wraps a parsed key. Status resolution is a stub (always Pro / 1 seat).
    #[must_use]
    pub fn new(key: LicenseKey) -> Self {
        Self { key }
    }

    /// The parsed key.
    #[must_use]
    pub fn key(&self) -> &LicenseKey {
        &self.key
    }
}

impl EntitlementChecker for LicenseChecker {
    fn status(&self) -> EntitlementStatus {
        // STUB: a well-formed key is treated as a Pro / 1-seat license. The
        // real backend will verify the Ed25519 signature offline and decode
        // { plan, seats, not_after } from the payload.
        EntitlementStatus::Licensed {
            plan: Plan::Pro,
            seats: 1,
        }
    }
}

/// Resolves the entitlement checker from the environment + CLI flag.
///
/// Priority: explicit `key` arg → `BITVANES_LICENSE_KEY` env → open-source.
/// Returns an `Arc<dyn EntitlementChecker>` suitable for sharing across
/// request handlers.
#[must_use]
pub fn resolve_checker(key: Option<&str>) -> Arc<dyn EntitlementChecker> {
    let raw = key
        .map(str::to_string)
        .or_else(|| std::env::var("BITVANES_LICENSE_KEY").ok())
        .filter(|s| !s.trim().is_empty());
    match raw.and_then(|r| LicenseKey::parse(&r).ok()) {
        Some(k) => Arc::new(LicenseChecker::new(k)),
        None => Arc::new(OpenSourceChecker),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_source_checker_is_permissive() {
        let c = OpenSourceChecker;
        assert!(matches!(c.status(), EntitlementStatus::OpenSource));
        assert!(c.status().allow_paid_features());
    }

    #[test]
    fn license_key_rejects_empty_and_wrong_prefix() {
        assert!(LicenseKey::parse("").is_err());
        assert!(LicenseKey::parse("nope").is_err());
        assert!(LicenseKey::parse("sk_live_xyz").is_err());
    }

    #[test]
    fn license_key_accepts_bv1_prefix() {
        let k = LicenseKey::parse("bv1_payload.sig").unwrap();
        assert_eq!(k.as_str(), "bv1_payload.sig");
    }

    #[test]
    fn license_checker_stub_returns_pro_one_seat() {
        let k = LicenseKey::parse("bv1_test").unwrap();
        let c = LicenseChecker::new(k);
        match c.status() {
            EntitlementStatus::Licensed { plan, seats } => {
                assert_eq!(plan, Plan::Pro);
                assert_eq!(seats, 1);
            }
            other => panic!("expected Licensed, got {other:?}"),
        }
        assert!(c.status().allow_paid_features());
    }

    #[test]
    fn invalid_status_denies_paid_features() {
        let s = EntitlementStatus::Invalid {
            reason: "expired".to_string(),
        };
        assert!(!s.allow_paid_features());
        assert!(s.label().contains("invalid"));
    }
}
