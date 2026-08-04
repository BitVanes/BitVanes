/**
 * Verify a PayPal capture and mint the license — `POST /api/v1/license/verify-and-mint`.
 *
 * Called by the checkout's `onApprove` immediately after the buyer approves.
 * Verifies the REAL PayPal order (status + amount) server-side, then mints +
 * returns the key. The customer sees the key instantly on the success page;
 * the webhook is a fallback if the client fails (browser closed, etc.).
 *
 * Body: `{ "orderID": "...", "email": "..." }`
 * Returns: `{ licenseKey, tier, exp }`
 */

import { mintLicense, tierForAmount } from '../../services/licenseSigner.js';
import { sendLicenseEmail } from '../../services/email.js';

const PAYPAL_BASE = process.env.PAYPAL_API_BASE || 'https://api-m.paypal.com';
const PAYPAL_CLIENT_ID = process.env.PAYPAL_CLIENT_ID || process.env.PAYPAL_ID || '';
const PAYPAL_SECRET = process.env.PAYPAL_SECRET || process.env.PAYPAL_CLIENT_SECRET || '';

async function paypalAccessToken() {
  const auth = Buffer.from(`${PAYPAL_CLIENT_ID}:${PAYPAL_SECRET}`).toString('base64');
  const res = await fetch(`${PAYPAL_BASE}/v1/oauth2/token`, {
    method: 'POST',
    headers: {
      Authorization: `Basic ${auth}`,
      'Content-Type': 'application/x-www-form-urlencoded',
    },
    body: 'grant_type=client_credentials',
  });
  if (!res.ok) throw new Error(`PayPal auth failed (${res.status})`);
  return (await res.json()).access_token;
}

export default async function handler(req, res) {
  if (req.method !== 'POST') {
    res.setHeader('Allow', 'POST');
    return res.status(405).end();
  }

  const { orderID, email } = req.body || {};
  if (!orderID || !email) {
    return res.status(400).json({ error: 'orderID and email required' });
  }

  try {
    // 1. Verify the PayPal order is captured.
    const token = await paypalAccessToken();
    const orderRes = await fetch(`${PAYPAL_BASE}/v2/checkout/orders/${orderID}`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    if (!orderRes.ok) {
      return res.status(400).json({ error: 'PayPal order lookup failed' });
    }
    const order = await orderRes.json();
    if (order.status !== 'COMPLETED') {
      return res.status(400).json({ error: `order status is ${order.status}, not COMPLETED` });
    }

    // 2. Amount → tier.
    const capture = order.purchase_units?.[0]?.payments?.captures?.[0];
    const amount = parseFloat(capture?.amount?.value || '0');
    const currency = capture?.amount?.currency_code;
    if (currency !== 'USD') {
      return res.status(400).json({ error: `non-USD: ${currency}` });
    }
    const tier = tierForAmount(amount);
    if (!tier) {
      return res.status(400).json({ error: `amount ${amount} does not map to a tier` });
    }

    // 3. Mint + best-effort email.
    const out = await mintLicense({
      privateKeyHex: process.env.BITVANES_LICENSE_PRIVATE_KEY,
      email,
      tier,
    });
    try {
      await sendLicenseEmail({ to: email, licenseKey: out.licenseKey, tier, expiresAt: out.exp });
    } catch (mailErr) {
      console.error('license email failed (mint succeeded):', String(mailErr.message));
    }
    console.log(`license minted: tier=${tier} sub=${email}`);

    // 4. Return the key to the client (success page displays it).
    return res.status(200).json(out);
  } catch (err) {
    console.error('verify-and-mint error:', err);
    return res
      .status(500)
      .json({ error: 'verify-and-mint failed', detail: String(err.message) });
  }
}
