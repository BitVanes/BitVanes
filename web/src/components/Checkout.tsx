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

// PayPal client IDs are public (they're embedded in browser SDK URLs). This is
// the SANDBOX id; set VITE_PAYPAL_CLIENT_ID to your LIVE id for production.
const PAYPAL_CLIENT_ID =
  import.meta.env.VITE_PAYPAL_CLIENT_ID ||
  'AQOpEbyMvmDD1q_E75lQ8utN86yyn-zJhpSH7RVxPf9Z0jh138iFqX_RFGmX1AEXmQqAVOoLqdSaG7K6';

const PRICES = { solo: '99.00', business: '399.00' };
const isSandbox = PAYPAL_CLIENT_ID.startsWith('AQ'); // sandbox ids typically start with AQ

type Result = { tier: 'solo' | 'business'; email: string };

export default function Checkout() {
  const [email, setEmail] = useState('');
  const [result, setResult] = useState<Result | null>(null);
  const validEmail = /^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(email);

  const createOrder = (tier: 'solo' | 'business') => (_data: unknown, actions: any) =>
    actions.order.create({
      intent: 'CAPTURE',
      purchase_units: [
        { amount: { value: PRICES[tier], currency_code: 'USD' }, custom_id: email },
      ],
    });

  const onApprove = (tier: 'solo' | 'business') => (_data: unknown, actions: any) =>
    actions.order.capture().then(() => setResult({ tier, email }));

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
              Your <strong>{result.tier}</strong> license is being generated and emailed to{' '}
              <code>{result.email}</code>.
            </p>
            <p className="checkout-instr">
              Activate it once it arrives:
              <br />
              <code>bitvanes config --key BV-{result.tier.toUpperCase()}-…</code>
            </p>
            {isSandbox && (
              <p className="checkout-sandbox">
                Sandbox mode — no real charge. If the email doesn’t arrive (sandbox senders are
                restricted), grab the key from <strong>Vercel → Functions → Logs</strong> (look for
                <code> license minted:</code>).
              </p>
            )}
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
                    onApprove={onApprove('solo')}
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
                    onApprove={onApprove('business')}
                  />
                )}
              </div>
            </div>
            {isSandbox && <p className="checkout-sandbox">⚠️ Sandbox mode — no real charges.</p>}
          </>
        )}
      </div>
    </PayPalScriptProvider>
  );
}
