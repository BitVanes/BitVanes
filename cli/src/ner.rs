//! CLI-side wiring for the Tier-2 NER sidecar (`bitvanes-nerd`).
//!
//! The decision to attach NER is made here, in the CLI, from two gates:
//! 1. **Entitlement** — [`EntitlementStatus::allows_ner`] (paid only). The free
//!    tier never gets name detection.
//! 2. **Availability** — the sidecar's socket must exist (`bitvanes-nerd` is
//!    running). NER is opt-in by running the sidecar; if it isn't running, the
//!    engine silently falls back to Tier-1 rather than fail every scrub.
//!
//! When both gates pass, a [`RemoteNerClient`] is attached to the [`Scrubber`]
//! via `with_detectors`; the scrubber then routes through the fail-closed
//! `scrub_with_detectors` path. The license token (a `bv1_…` key) is forwarded
//! to the sidecar, which re-verifies it offline (defense in depth).

use std::path::PathBuf;
use std::sync::Arc;

use bitvanes_core::pii::Scrubber;
use bitvanes_core::pii::ner_client::unix::UnixSocketTransport;
use bitvanes_core::pii::RemoteNerClient;

use crate::entitlement::EntitlementStatus;

/// Resolve the sidecar socket path: `--ner-socket` flag, else
/// `BITVANES_NERD_SOCKET`, else `$XDG_RUNTIME_DIR/bitvanes-nerd.sock`
/// (or `/tmp/...`). Matches `bitvanes-nerd`'s own default so a vanilla
/// `bitvanes-nerd` + `bitvanes daemon` pair connect with no config.
#[must_use]
pub fn resolve_socket(override_path: Option<&str>) -> PathBuf {
    if let Some(p) = override_path {
        return PathBuf::from(p);
    }
    if let Ok(p) = std::env::var("BITVANES_NERD_SOCKET") {
        return PathBuf::from(p);
    }
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    dir.join("bitvanes-nerd.sock")
}

/// The license token to forward to the sidecar: `BITVANES_LICENSE_KEY` when set
/// (the same key the CLI verifies locally via [`crate::entitlement`]).
#[must_use]
pub fn license_token() -> Option<String> {
    std::env::var("BITVANES_LICENSE_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Attach the Tier-2 NER sidecar to `scrubber` if (a) the entitlement allows it
/// and (b) the sidecar socket exists. Otherwise return the scrubber unchanged
/// (Tier-1 only).
///
/// This is the single decision point for NER in the CLI. `socket_override`
/// is the `--ner-socket` flag value; `token` is the [`license_token()`].
#[must_use]
pub fn try_attach(
    scrubber: Scrubber,
    status: &EntitlementStatus,
    socket_override: Option<&str>,
    token: &Option<String>,
) -> Scrubber {
    // Gate 1: paid entitlement. Free tier never gets NER, even if the socket
    // exists — enforcement is the license, not the sidecar's presence.
    if !status.allows_ner() {
        return scrubber;
    }

    let socket = resolve_socket(socket_override);

    // Gate 2: sidecar must be running. If the socket is absent, fall back to
    // Tier-1 silently (NER is opt-in by running `bitvanes-nerd`). If the socket
    // IS present but the sidecar later dies, the engine fails closed at scrub
    // time rather than emitting unredacted text.
    if !socket.exists() {
        return scrubber;
    }

    let transport = UnixSocketTransport::new(&socket);
    let client = RemoteNerClient::new(Box::new(transport), token.clone());
    eprintln!(
        "BitVanes NER: attached sidecar at {} (entitlement: {})",
        socket.display(),
        status.label()
    );
    scrubber.with_detectors(vec![Arc::new(client)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entitlement::Plan;
    use bitvanes_core::schema::ScrubProfile;

    fn pro_status() -> EntitlementStatus {
        EntitlementStatus::Licensed {
            plan: Plan::Pro,
            sub: "test-sub".to_string(),
            exp: i64::MAX,
        }
    }

    fn empty_scrubber() -> Scrubber {
        Scrubber::from_profile(&ScrubProfile::default()).expect("default profile compiles")
    }

    #[test]
    fn free_tier_never_attaches_even_with_socket_present() {
        // Free / open-source tier must NEVER get NER, regardless of socket.
        let socket = tempfile_path();
        std::fs::write(&socket, b"fake-socket").unwrap();
        let s = try_attach(
            empty_scrubber(),
            &EntitlementStatus::OpenSource,
            Some(socket.to_str().unwrap()),
            &None,
        );
        assert!(!s.has_detectors(), "free tier must not get a detector");
        let _ = std::fs::remove_file(&socket);
    }

    #[test]
    fn paid_tier_without_socket_does_not_attach() {
        // Entitled, but sidecar not running → no detector (Tier-1 fallback).
        let s = try_attach(
            empty_scrubber(),
            &pro_status(),
            Some("/this/socket/does/not/exist.sock"),
            &None,
        );
        assert!(!s.has_detectors(), "no socket → no detector");
    }

    #[test]
    fn invalid_license_never_attaches() {
        let socket = tempfile_path();
        std::fs::write(&socket, b"fake-socket").unwrap();
        let s = try_attach(
            empty_scrubber(),
            &EntitlementStatus::Invalid {
                reason: "expired".into(),
            },
            Some(socket.to_str().unwrap()),
            &None,
        );
        assert!(!s.has_detectors(), "invalid license → no detector");
        let _ = std::fs::remove_file(&socket);
    }

    #[test]
    fn paid_tier_with_socket_present_attaches() {
        // Paid + socket present → detector attached, token forwarded.
        let socket = tempfile_path();
        std::fs::write(&socket, b"fake-socket").unwrap();
        let token = Some("bv1_test_token".to_string());
        let s = try_attach(
            empty_scrubber(),
            &pro_status(),
            Some(socket.to_str().unwrap()),
            &token,
        );
        assert!(s.has_detectors(), "paid + socket → detector attached");
        let _ = std::fs::remove_file(&socket);
    }

    #[test]
    fn socket_resolution_prefers_override_and_defaults_sensibly() {
        // explicit override always wins
        assert_eq!(
            resolve_socket(Some("/explicit/sock")),
            PathBuf::from("/explicit/sock")
        );
        // with no override and no `BITVANES_NERD_SOCKET` env (the common test
        // env), the default ends in the conventional filename. (The env-var
        // branch is a trivial `std::env::var` and isn't worth the global-mutation
        // hazard of testing it here.)
        if std::env::var_os("BITVANES_NERD_SOCKET").is_none() {
            assert!(resolve_socket(None).ends_with("bitvanes-nerd.sock"));
        }
    }

    /// Unique temp path (no `tempfile` dep): `/tmp/bitvanes-cli-ner-test-<pid>-<n>`.
    fn tempfile_path() -> PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        PathBuf::from(format!(
            "/tmp/bitvanes-cli-ner-test-{}-{n}.sock",
            std::process::id()
        ))
    }
}
