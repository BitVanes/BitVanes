# BitVanes license backend (Vercel Serverless Functions)

Mints + delivers Ed25519-signed license keys when a payment lands. Lives under
`web/api/` and deploys with the Vite frontend (Vercel auto-detects `api/`
serverless functions — no separate service to run).

## Endpoints

| Method | Path                       | Purpose                                             |
|--------|----------------------------|-----------------------------------------------------|
| POST   | `/api/webhooks/paypal`     | PayPal webhook → verify signature → mint + email    |
| POST   | `/api/v1/license/x402`     | x402 USDC 402-handshake → mint + return JSON        |

## Architecture

```
PayPal capture ─┐                                              ┌─▶ email (Resend)
                ├─▶ licenseSigner.mintLicense() ─▶ BV-TIER-JWT ─┤
x402 payment ───┘   (Ed25519 sign)                              └─▶ HTTP 200 JSON
                                                                     (x402 path)
```

The CLI verifies the resulting `BV-{TIER}-…` key **offline** against a public
key embedded in the binary (`cli/src/entitlement.rs`). No call home, no
revocation server — expiry is baked into the signed JWT payload.

## Setup

1. **Generate the keypair** (one-time, deterministic dev keypair by default):
   ```bash
   cd cli && cargo run --example keygen
   ```
   - Embed the printed **public key** in `cli/src/entitlement.rs` (`DEV_PUBKEY`).
   - Set the printed **private key** as `BITVANES_LICENSE_PRIVATE_KEY` in Vercel.

   The current dev keypair is deterministic — `cargo run --example keygen`
   prints the public key (to embed) **and** the private key (set as the Vercel
   env var). The private key must NEVER be committed; this README does not
   reproduce it. Run the command to obtain it for local testing.

   **For production**: change the seed in `examples/keygen.rs` (or use `OsRng`),
   re-embed the new public key, rebuild the CLI, and set the new private key in
   Vercel. Rotate if the private key ever leaks.

2. **Set the env vars** (Vercel → Settings → Environment Variables). See
   [`../.env.example`](../.env.example) for the full list.

3. **PayPal webhook**: create a webhook in the PayPal dashboard for
   `PAYMENT.CAPTURE.COMPLETED`, pointing at `https://bitvanes.com/api/webhooks/paypal`.
   Set the webhook ID as `PAYPAL_WEBHOOK_ID`.

4. **x402**: set `BITVANES_USDC_RECEIVER_WALLET` to your USDC-receiving EVM
   address on the configured network (Base by default). Confirm the facilitator
   request/response shape against the installed `@x402/*` package version before
   going live — the protocol-level handler in `v1/license/x402.js` follows the
   documented x402 spec.

## Verified vs. needs-deployment-verification

- ✅ **`licenseSigner.js`** — verified end-to-end: a JS-minted key (dev privkey)
  is accepted by the Rust CLI's offline verifier (embedded pubkey).
- ⚠️ **PayPal webhook signature verification + x402 facilitator handshake** —
  follow the documented APIs but require your live credentials + a test
  transaction to fully verify before going live.
