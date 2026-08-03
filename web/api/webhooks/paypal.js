/**
 * PayPal webhook listener — `POST /api/webhooks/paypal`.
 *
 * Verifies the webhook signature via the PayPal REST API, then mints + emails
 * a license key based on the captured amount.
 *
 * Required env:
 *   PAYPAL_CLIENT_ID
 *   PAYPAL_CLIENT_SECRET
 *   PAYPAL_WEBHOOK_ID
 *   PAYPAL_API_BASE        (default: https://api-m.paypal.com)
 *   BITVANES_LICENSE_PRIVATE_KEY  (Ed25519, 32-byte hex)
 *   RESEND_API_KEY / LICENSE_EMAIL_FROM (for email delivery)
 *
 * Vercel Serverless Function (Node 18+ for native fetch).
 */

import { mintLicense, tierForAmount } from '../services/licenseSigner.js';
import { sendLicenseEmail } from '../services/email.js';

const PAYPAL_BASE = process.env.PAYPAL_API_BASE || 'https://api-m.paypal.com';
// Tolerant of either PAYPAL_SECRET or PAYPAL_CLIENT_SECRET (and PAYPAL_ID /
// PAYPAL_CLIENT_ID) so the env var names you set work without renaming.
const PAYPAL_CLIENT_ID =
  process.env.PAYPAL_CLIENT_ID || process.env.PAYPAL_ID || '';
const PAYPAL_SECRET =
  process.env.PAYPAL_SECRET || process.env.PAYPAL_CLIENT_SECRET || '';

/** Exchange client credentials for a PayPal access token. */
async function paypalAccessToken() {
  const auth = Buffer.from(`${PAYPAL_CLIENT_ID}:${PAYPAL_SECRET}`).toString('base64');
  const res = await fetch(`${PAYPAL_BASE}/v1/oauth2/token`, {
    method: 'POST',
    headers: { Authorization: `Basic ${auth}`, 'Content-Type': 'application/x-www-form-urlencoded' },
    body: 'grant_type=client_credentials',
  });
  if (!res.ok) throw new Error(`PayPal auth failed (${res.status})`);
  return (await res.json()).access_token;
}

/** Verify the incoming webhook signature with PayPal. */
async function verifyWebhookSignature(headers, rawBody) {
  const token = await paypalAccessToken();
  const verification = {
    auth_algo: headers['paypal-auth-algo'],
    cert_url: headers['paypal-cert-url'],
    transmission_id: headers['paypal-transmission-id'],
    transmission_sig: headers['paypal-transmission-sig'],
    transmission_time: headers['paypal-transmission-time'],
    webhook_id: process.env.PAYPAL_WEBHOOK_ID,
    webhook_event: JSON.parse(rawBody),
  };
  const res = await fetch(`${PAYPAL_BASE}/v1/notifications/verify-webhook-signature`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify(verification),
  });
  if (!res.ok) throw new Error(`PayPal verify call failed (${res.status})`);
  return (await res.json()).verification_status === 'SUCCESS';
}

export default async function handler(req, res) {
  if (req.method !== 'POST') {
    res.setHeader('Allow', 'POST');
    return res.status(405).end('method not allowed');
  }

  // Vercel gives us the raw body when `webhooks.paypal.rawBody` is configured;
  // fall back to re-serializing the parsed body.
  const rawBody = typeof req.body === 'string' ? req.body : JSON.stringify(req.body);

  try {
    // 1. Verify the signature (reject spoofed webhooks).
    const ok = await verifyWebhookSignature(req.headers, rawBody);
    if (!ok) return res.status(400).json({ error: 'invalid webhook signature' });

    const event = JSON.parse(rawBody);
    if (event.event_type !== 'PAYMENT.CAPTURE.COMPLETED') {
      return res.status(200).json({ ignored: event.event_type });
    }

    // 2. Determine tier from the captured amount.
    const amount = parseFloat(event.resource?.amount?.value ?? '0');
    const currency = event.resource?.amount?.currency_code;
    if (currency !== 'USD') {
      return res.status(200).json({ ignored: `non-USD currency: ${currency}` });
    }
    const tier = tierForAmount(amount);
    if (!tier) {
      return res.status(400).json({ error: `amount ${amount} does not map to a tier` });
    }

    // 3. Mint + email the license.
    const email =
      event.resource?.custom_id ||
      event.resource?.payer?.email_address ||
      event.resource?.shipping?.email_address;
    if (!email) {
      return res.status(400).json({ error: 'no customer email in webhook payload' });
    }

    const { licenseKey, exp } = await mintLicense({
      privateKeyHex: process.env.BITVANES_LICENSE_PRIVATE_KEY,
      email,
      tier,
    });
    await sendLicenseEmail({ to: email, licenseKey, tier, expiresAt: exp });

    return res.status(200).json({ success: true, tier, email, exp });
  } catch (err) {
    console.error('PayPal webhook error:', err);
    return res.status(500).json({ error: 'internal error', detail: String(err.message) });
  }
}
