/**
 * Dev-only license minter — `POST /api/v1/license/dev-mint`.
 *
 * Mints a real, CLI-verifiable license key WITHOUT a payment. Locked behind a
 * secret token (`BITVANES_DEV_MINT_TOKEN`) so it can't mint free keys unless
 * the caller holds the token. Use for sandbox testing + manual issuance.
 *
 * **REMOVE this file (or unset BITVANES_DEV_MINT_TOKEN) before live payments.**
 *
 * Header:  `x-dev-mint-token: <BITVANES_DEV_MINT_TOKEN>`
 * Body:    `{ "email": "...", "tier": "solo"|"business", "durationDays": 365 }`
 * Returns: `{ licenseKey, tier, exp }`
 */

import { mintLicense } from '../../services/licenseSigner.js';

export default async function handler(req, res) {
  if (req.method !== 'POST') {
    res.setHeader('Allow', 'POST');
    return res.status(405).end('method not allowed');
  }

  const expected = process.env.BITVANES_DEV_MINT_TOKEN;
  if (!expected) {
    return res.status(404).end(); // endpoint disabled in this deployment
  }
  const got = req.headers['x-dev-mint-token'];
  if (!got || got.length < 16 || got !== expected) {
    return res.status(403).json({ error: 'forbidden' });
  }

  try {
    const { email, tier = 'solo', durationDays = 365 } = req.body || {};
    if (!email) return res.status(400).json({ error: 'email required' });

    const out = await mintLicense({
      privateKeyHex: process.env.BITVANES_LICENSE_PRIVATE_KEY,
      email,
      tier,
      durationDays,
    });
    return res.status(200).json(out);
  } catch (err) {
    return res.status(500).json({ error: 'mint failed', detail: String(err.message) });
  }
}
