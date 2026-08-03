/**
 * Transactional email — license-key delivery.
 *
 * Uses Resend (https://resend.com). Swap the transport for SendGrid/Postmark
 * by replacing this module; the signature stays the same.
 *
 * Server-only. Requires `RESEND_API_KEY` and `LICENSE_EMAIL_FROM`.
 */

const RESEND_ENDPOINT = 'https://api.resend.com/emails';

/**
 * @param {object} opts
 * @param {string} opts.to        — Customer email.
 * @param {string} opts.licenseKey — `BV-{TIER}-…` key.
 * @param {string} opts.tier      — `'solo' | 'business'`.
 * @param {string} opts.expiresAt  — ISO 8601 expiry.
 */
export async function sendLicenseEmail({ to, licenseKey, tier, expiresAt }) {
  const apiKey = process.env.RESEND_API_KEY;
  const from = process.env.LICENSE_EMAIL_FROM || 'licenses@bitvanes.com';
  if (!apiKey) {
    console.error('RESEND_API_KEY not set; skipping license email to', to);
    return { skipped: true };
  }

  const html = `<!doctype html><html><body style="font-family:system-ui,sans-serif;max-width:560px;margin:40px auto;color:#222">
  <h2>Your BitVanes ${tier === 'business' ? 'Business' : 'Solo'} license</h2>
  <p>Thanks for purchasing BitVanes! Your license key (valid until ${expiresAt}):</p>
  <pre style="background:#f4f4f5;padding:16px;border-radius:8px;font-size:14px;word-break:break-all">${licenseKey}</pre>
  <h3>Activate in 30 seconds</h3>
  <ol>
    <li><a href="https://github.com/BitVanes/BitVanes/releases">Download</a> the <code>bitvanes</code> binary.</li>
    <li>Install your key: <code>bitvanes config --key ${licenseKey.slice(0, 24)}…</code></li>
    <li>Start scrubbing: <code>bitvanes scrub ./docs --pdf-mode redact</code></li>
  </ol>
  <p style="color:#666;font-size:13px">Keep this key private. It works offline — no call home. Questions? reply to this email.</p>
</body></html>`;

  const res = await fetch(RESEND_ENDPOINT, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${apiKey}`,
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({
      from,
      to: [to],
      subject: `Your BitVanes ${tier} license key`,
      html,
    }),
  });
  if (!res.ok) {
    const detail = await res.text();
    throw new Error(`Resend email failed (${res.status}): ${detail}`);
  }
  return { skipped: false, id: (await res.json()).id };
}
