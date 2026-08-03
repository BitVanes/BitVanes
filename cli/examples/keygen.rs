//! Offline Ed25519 license keypair generator + sample-key minter.
//!
//! Run with: `cargo run --example keygen`
//!
//! Uses a cryptographically-random keypair (OsRng) each run — there is NO
//! committed seed, so the private key is never recoverable from the repo.
//!
//! Prints:
//!   - the 32-byte **public key** as a Rust array literal (embed this in
//!     `src/entitlement.rs` as `LICENSE_PUBKEY`),
//!   - the 32-byte **private key** as hex (set this as
//!     `BITVANES_LICENSE_PRIVATE_KEY` on the minting backend; NEVER commit it),
//!   - a sample `BV-SOLO-<jwt>` license signed by that key (for testing).

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use serde_json::json;

fn b64u(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn mint_license(privkey: &SigningKey, sub: &str, tier: &str, exp_unix: i64) -> String {
    let header = b64u(br#"{"alg":"EdDSA","typ":"JWT"}"#);
    let payload = json!({
        "sub": sub,
        "tier": tier,
        "iss": "bitvanes.com",
        "iat": 1_700_000_000_i64,
        "exp": exp_unix,
    });
    let payload_bytes = serde_json::to_vec(&payload).unwrap();
    let payload_b64 = b64u(&payload_bytes);
    let signing_input = format!("{header}.{payload_b64}");
    let sig = privkey.sign(signing_input.as_bytes());
    format!(
        "BV-{}-{}.{}",
        tier.to_uppercase(),
        signing_input,
        b64u(&sig.to_bytes())
    )
}

fn main() {
    // Cryptographically random keypair — generated once, never reproducible
    // from the repo. Embed the public key in the CLI; keep the private key in
    // the backend secret store only.
    let mut rng = OsRng;
    let signing_key = SigningKey::generate(&mut rng);
    let verifying_key = signing_key.verifying_key();

    let pub_bytes = verifying_key.to_bytes();
    let priv_bytes = signing_key.to_bytes();

    // Rust array literal for embedding.
    let pub_arr: Vec<String> = pub_bytes.iter().map(|b| format!("0x{b:02x}")).collect();
    println!("=== PUBLIC KEY (embed in src/entitlement.rs DEV_PUBKEY) ===");
    println!("[{}]", pub_arr.join(", "));
    println!();

    println!("=== PRIVATE KEY (set as BITVANES_LICENSE_PRIVATE_KEY on the backend) ===");
    println!("{}", hex_encode(&priv_bytes));
    println!();

    let one_year = 1_700_000_000_i64 + 365 * 86_400;
    let _ = one_year; // (kept for reference; sample below uses a far-future exp)
    let license = mint_license(&signing_key, "customer@example.com", "solo", 1_900_000_000);
    println!("=== SAMPLE BV-SOLO LICENSE (valid 365d) ===");
    println!("{license}");
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
