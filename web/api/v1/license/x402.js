/**
 * x402 agentic payment endpoint — `POST /api/v1/license/x402`.
 *
 * Implements the x402 HTTP 402 payment protocol at the protocol level:
 *   1. No `X-PAYMENT` header → respond 402 with payment requirements.
 *   2. `X-PAYMENT` header present → verify via the facilitator; on success,
 *      mint + return the license key in a 200 JSON body.
 *
 * Query params: `?tier=solo|business`.
 *   solo     → $99.00 USDC (annual)
 *   business → $399.00 USDC (annual)
 *
 * Required env:
 *   BITVANES_USDC_RECEIVER_WALLET  — EVM address receiving USDC.
 *   X402_FACILITATOR_URL           — default: https://x402.org/facilitator
 *   X402_NETWORK                   — default: base  (base | avalanche)
 *   BITVANES_LICENSE_PRIVATE_KEY   — Ed25519, 32-byte hex.
 *
 * NOTE: verify the facilitator request/response shape against the installed
 * `@x402/*` / `@coinbase/x402` package version before going live; the
 * protocol-level handlers below follow the documented x402 spec.
 */

import { mintLicense } from '../../services/licenseSigner.js';

const PRICES = { solo: '99.00', business: '399.00' };
const FACILITATOR = process.env.X402_FACILITATOR_URL || 'https://x402.org/facilitator';
const NETWORK = process.env.X402_NETWORK || 'base';

function requirements(tier) {
  return {
    scheme: 'exact',
    network: NETWORK,
    asset: '@base/USDC',
    price: PRICES[tier],
    payTo: process.env.BITVANES_USDC_RECEIVER_WALLET,
    description: `BitVanes ${tier} license (annual)`,
    mimeType: 'application/json',
    maxTimeoutSeconds: 60,
  };
}

/** Ask the facilitator to verify a payment; returns { valid, ... }. */
async function verifyPayment(paymentHeader, reqs) {
  let paymentPayload;
  try {
    paymentPayload = JSON.parse(Buffer.from(paymentHeader, 'base64').toString());
  } catch {
    return { valid: false, reason: 'malformed X-PAYMENT header' };
  }
  const res = await fetch(`${FACILITATOR}/verify`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ paymentPayload, requirements: reqs }),
  });
  if (!res.ok) return { valid: false, reason: `facilitator ${res.status}` };
  const body = await res.json();
  return body; // { valid: boolean, ... }
}

export default async function handler(req, res) {
  if (req.method !== 'POST') {
    res.setHeader('Allow', 'POST');
    return res.status(405).end('method not allowed');
  }

  const tier = req.query?.tier === 'business' ? 'business' : 'solo';
  if (!process.env.BITVANES_USDC_RECEIVER_WALLET) {
    return res.status(500).json({ error: 'BITVANES_USDC_RECEIVER_WALLET not configured' });
  }

  const reqs = requirements(tier);

  // Step 1: no payment yet → 402 with requirements.
  const paymentHeader = req.headers['x-payment'];
  if (!paymentHeader) {
    res.setHeader('WWW-Authenticate', 'x402');
    return res.status(402).json({
      x402Version: 1,
      error: 'payment_required',
      accepts: reqs,
    });
  }

  // Step 2: verify the payment via the facilitator.
  try {
    const verification = await verifyPayment(paymentHeader, reqs);
    if (!verification.valid) {
      res.setHeader('WWW-Authenticate', 'x402');
      return res.status(402).json({
        x402Version: 1,
        error: 'payment_invalid',
        accepts: reqs,
        detail: verification.reason,
      });
    }

    // Step 3: payment verified — mint + return the license.
    const sub = verification.payerAddress || verification.sub || 'x402-agent';
    const { licenseKey, exp } = await mintLicense({
      privateKeyHex: process.env.BITVANES_LICENSE_PRIVATE_KEY,
      email: sub,
      tier,
    });

    return res.status(200).json({
      success: true,
      tier,
      licenseKey,
      expiresAt: exp,
    });
  } catch (err) {
    console.error('x402 endpoint error:', err);
    return res.status(500).json({ error: 'internal error', detail: String(err.message) });
  }
}
