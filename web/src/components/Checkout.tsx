import { useState } from 'react';
import { PayPalScriptProvider, PayPalButtons } from '@paypal/react-paypal-js';

/**
 * PayPal checkout — renders Solo ($99/yr) and Business ($399/yr) buttons.
 *
 * On capture, PayPal fires the webhook at /api/webhooks/paypal, which verifies
 * the signature, mints the license key, and emails it to the buyer. The client
 * just reports success.
 *
 * `clientId` is the public PayPal client-id (safe in the browser). Sandbox by
 * default; override with VITE_PAYPAL_CLIENT_ID for live.
 */

// PayPal client IDs are public (embedded in browser SDK URLs).
// Set VITE_PAYPAL_CLIENT_ID in Vercel to your live ID.
const PAYPAL_CLIENT_ID = import.meta.env.VITE_PAYPAL_CLIENT_ID || '';

const PRICES = { solo: '99.00', business: '399.00' };

type Result = { tier: 'solo' | 'business'; email: string; licenseKey: string; exp: string };

export default function Checkout() {
  const [email, setEmail] = useState('');
  const [result, setResult] = useState<Result | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const validEmail = /^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(email);

  const createOrder = (tier: 'solo' | 'business') => (_data: unknown, actions: any) =>
    actions.order.create({
      intent: 'CAPTURE',
      purchase_units: [
        { amount: { value: PRICES[tier], currency_code: 'USD' }, custom_id: email },
      ],
    });

  const onApprove = (_data: { orderID: string }, _actions: any) =>
    // The SDK captures the order; then we verify + mint server-side.
    _actions.order.capture().then(async () => {
      setError(null);
      try {
        const resp = await fetch('/api/v1/license/verify-and-mint', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ orderID: _data.orderID, email }),
        });
        if (!resp.ok) {
          const err = await resp.json().catch(() => ({}));
          throw new Error(err.error || `server returned ${resp.status}`);
        }
        const data = await resp.json();
        setResult({ tier: data.tier, email, licenseKey: data.licenseKey, exp: data.exp });
      } catch (e) {
        setError(
          'Payment captured but key delivery failed. Check your email — the backup webhook will mint + send it.',
        );
      }
    });

  const copyKey = () => {
    if (result) {
      navigator.clipboard.writeText(result.licenseKey);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  if (!PAYPAL_CLIENT_ID) {
    return <p className="checkout-error">PayPal not configured (set VITE_PAYPAL_CLIENT_ID).</p>;
  }

  return (
    <PayPalScriptProvider
      options={{ clientId: PAYPAL_CLIENT_ID, currency: 'USD', intent: 'capture' }}
    >
      <div className="checkout">
        {result ? (
          <div className="checkout-success">
            <h4>Payment received 🎉</h4>
            <p>
              Your <strong>{result.tier}</strong> license is ready. Copy this key
              and activate it:
            </p>
            <div className="checkout-key-box">
              <code className="checkout-key">{result.licenseKey}</code>
              <button className="btn-outline btn-sm" onClick={copyKey}>
                {copied ? '✓ Copied' : 'Copy'}
              </button>
            </div>
            <p className="checkout-instr">
              Paste it in your terminal:
              <br />
              <code>bitvanes config --key BV-{result.tier.toUpperCase()}-…</code>
            </p>
            <p className="checkout-sandbox">
              A copy was also emailed to <code>{result.email}</code>.
            </p>
            <button className="btn-outline btn-sm" onClick={() => setResult(null)}>
              Pay again
            </button>
          </div>
        ) : (
          <>
            <label htmlFor="ck-email">Email (your key is sent here)</label>
            <input
              id="ck-email"
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="you@example.com"
              autoComplete="email"
            />
            {!validEmail && <p className="checkout-hint">Enter your email to enable payment.</p>}
            <div className="checkout-buttons">
              <div className="checkout-tier">
                <div className="checkout-tier-head">
                  <strong>Solo</strong> <span>$99 lifetime</span>
                </div>
                {validEmail && (
                  <PayPalButtons
                    style={{ layout: 'vertical', label: 'pay', height: 40 }}
                    fundingSource="paypal"
                    createOrder={createOrder('solo')}
                    onApprove={onApprove}
                  />
                )}
              </div>
              <div className="checkout-tier">
                <div className="checkout-tier-head">
                  <strong>Business</strong> <span>$399 lifetime</span>
                </div>
                {validEmail && (
                  <PayPalButtons
                    style={{ layout: 'vertical', label: 'pay', height: 40 }}
                    fundingSource="paypal"
                    createOrder={createOrder('business')}
                    onApprove={onApprove}
                  />
                )}
              </div>
            </div>
          </>
        )}
      </div>
    </PayPalScriptProvider>
  );
}
