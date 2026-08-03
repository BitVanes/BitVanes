/**
 * License key minting — the signing counterpart to the CLI's offline verifier.
 *
 * Produces `BV-{TIER}-{header.payload.signature}` JWT strings signed with
 * Ed25519, mirroring `cli/examples/keygen.rs` and `cli/src/entitlement.rs`
 * exactly. The CLI verifies these offline against the embedded public key.
 *
 * NEVER ship the private key in a client bundle. This module is server-only
 * (Vercel Serverless Function), reading the key from process.env.
 */

import { signAsync } from '@noble/ed25519';

const ISSUER = 'bitvanes.com';
const DEFAULT_DURATION_DAYS = 365;

/** Decode a hex string to a Uint8Array. */
function hexToBytes(hex) {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(hex.substr(i * 2, 2), 16);
  }
  return out;
}

/** Base64url-encode a JSON-serializable object (no padding). */
function b64uJson(obj) {
  return Buffer.from(JSON.stringify(obj)).toString('base64url');
}

/**
 * Mint a signed license key.
 *
 * @param {object} opts
 * @param {string} opts.privateKeyHex — Ed25519 private key (32 bytes, hex),
 *   from `BITVANES_LICENSE_PRIVATE_KEY`.
 * @param {string} opts.email        — Customer identifier (JWT `sub`).
 * @param {'solo'|'business'} opts.tier — License tier.
 * @param {number} [opts.durationDays=365] — Validity window.
 * @returns {Promise<{licenseKey: string, tier: string, exp: string}>}
 */
export async function mintLicense({ privateKeyHex, email, tier, durationDays = DEFAULT_DURATION_DAYS }) {
  if (!privateKeyHex) throw new Error('BITVANES_LICENSE_PRIVATE_KEY is not set');
  if (tier !== 'solo' && tier !== 'business') {
    throw new Error(`invalid tier '${tier}' (expected solo|business)`);
  }

  const iat = Math.floor(Date.now() / 1000);
  const exp = iat + durationDays * 86_400;

  const header = b64uJson({ alg: 'EdDSA', typ: 'JWT' });
  const payload = b64uJson({ sub: email, tier, iss: ISSUER, iat, exp });
  const signingInput = `${header}.${payload}`;

  const privateKey = hexToBytes(privateKeyHex);
  const message = new TextEncoder().encode(signingInput);
  const signature = await signAsync(message, privateKey);
  const sigB64 = Buffer.from(signature).toString('base64url');

  const licenseKey = `BV-${tier.toUpperCase()}-${signingInput}.${sigB64}`;
  return {
    licenseKey,
    tier,
    exp: new Date(exp * 1000).toISOString(),
  };
}

/** Amount → tier mapping (per AGENT DIRECTIVE). */
export function tierForAmount(usd) {
  if (usd >= 49) return 'business';
  if (usd >= 12) return 'solo';
  return null;
}
