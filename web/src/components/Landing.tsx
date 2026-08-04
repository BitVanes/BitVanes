import { useState } from 'react';
import Checkout from './Checkout';
import DownloadButtons from './DownloadButtons';
import { ErrorBoundary } from './ErrorBoundary';
import LiveDemo from './LiveDemo';

const REPO = 'https://github.com/BitVanes/BitVanes';
const RELEASES = 'https://github.com/BitVanes/BitVanes/releases';

/**
 * Public landing page for bitvanes.com.
 * Problem-first, warm, plain language. The product is local-first PII
 * redaction; the page should make the pain obvious and the relief immediate.
 */
export default function Landing({ onOpenDashboard }: { onOpenDashboard: () => void }) {
  return (
    <div className="landing">
      <Nav onOpenDashboard={onOpenDashboard} />
      <Hero />
      <ErrorBoundary><LiveDemo /></ErrorBoundary>
      <Problems />
      <VsIncumbents />
      <DemoStrip />
      <HowItWorks />
      <Features />
      <Pricing />
      <ForAgents />
      <ZeroTrustCallout />
      <Footer />
    </div>
  );
}

function Nav({ onOpenDashboard }: { onOpenDashboard: () => void }) {
  return (
    <nav className="landing-nav">
      <a href="#top" className="logo">
        Bit<span style={{ color: 'var(--accent)' }}>Vanes</span>
      </a>
      <div className="nav-links">
        <a href="#demo">Live demo</a>
        <a href="#how">How it works</a>
        <a href="#pricing">Pricing</a>
        <a href={REPO} target="_blank" rel="noreferrer">
          GitHub
        </a>
        <a className="btn-primary btn-sm" href={RELEASES} target="_blank" rel="noreferrer">
          Download
        </a>
      </div>
    </nav>
  );
}

function Hero() {
  return (
    <header className="landing-hero" id="top">
      <span className="eyebrow">Local-first · Nothing leaves your machine</span>
      <h1>
        Share the document.
        <br />
        <span className="hl">Not the people in it.</span>
      </h1>
      <p className="subhead">
        BitVanes finds and removes names, SSNs, card numbers, and secrets from
        any file — in seconds, right on your laptop. Hand it a messy document,
        get back a clean one you can actually share.
      </p>
      <DownloadButtons />
      <p className="hero-foot">Free forever for text &amp; core PII. No account, no cloud.</p>
    </header>
  );
}

/** Relatable pain — the "why you need this" section. */
function Problems() {
  const pains = [
    {
      emoji: '📑',
      title: 'A vendor needs the contract',
      body: "…but it's full of customer names, account numbers, and signatures. You can't just send it as-is.",
    },
    {
      emoji: '🤖',
      title: 'Your team wants to use AI',
      body: '…but pasting patient records, legal files, or customer data into a chatbot is a compliance nightmare.',
    },
    {
      emoji: '🔍',
      title: 'An auditor wants the logs',
      body: '…and they’re packed with emails, SSNs, and API keys you are not allowed to hand over in the raw.',
    },
  ];
  return (
    <section className="problems">
      <h2>Sound familiar?</h2>
      <p className="section-sub">Redacting by hand takes hours. Skipping it risks a breach. There's a third option.</p>
      <div className="pain-grid">
        {pains.map((p) => (
          <div key={p.title} className="pain-card">
            <div className="pain-emoji">{p.emoji}</div>
            <h3>{p.title}</h3>
            <p>{p.body}</p>
          </div>
        ))}
      </div>
      <div className="pain-punchline">
        <span className="pain-strike">Hours of manual blackout</span> or{' '}
        <span className="pain-strike">a compliance incident</span>
        <br />
        — or <strong>30 seconds with BitVanes</strong>.
      </div>
    </section>
  );
}

/** Direct comparison — why BitVanes instead of the incumbent tools. */
function VsIncumbents() {
  const rows = [
    { feature: 'Runs locally — data never leaves your machine', bitvanes: true, adobe: 'Add-on, requires upload', purview: 'Cloud-dependent' },
    { feature: 'Destructive redaction (PII bytes deleted, not painted over)', bitvanes: true, adobe: 'Partial', purview: 'Labels only' },
    { feature: 'PDF + Word + Excel + PPTX + JSON + logs', bitvanes: true, adobe: 'PDF focus', purview: 'M365 focus' },
    { feature: 'Stream filtering (pipe stdin → stdout)', bitvanes: true, adobe: false, purview: false },
    { feature: 'CLI + daemon — scriptable, automatable', bitvanes: true, adobe: false, purview: 'API (enterprise tier)' },
    { feature: 'No account, no cloud, no telemetry', bitvanes: true, adobe: false, purview: false },
    { feature: 'Price', bitvanes: '$99 once', adobe: '$20+/mo', purview: '$12+/user/mo' },
  ];
  return (
    <section className="vs-incumbents">
      <h2>BitVanes vs. the usual suspects</h2>
      <p className="section-sub">
        Adobe Pro and Microsoft Purview are built for the cloud. BitVanes is
        built for the 80% of documents that should never touch one.
      </p>
      <div className="vs-table-wrap">
        <table className="vs-table">
          <thead>
            <tr>
              <th></th>
              <th className="vs-bitvanes">BitVanes</th>
              <th>Adobe Pro</th>
              <th>MS Purview</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.feature}>
                <td className="vs-feature">{r.feature}</td>
                <td className="vs-bitvanes">
                  {r.bitvanes === true ? '✅' : r.bitvanes}
                </td>
                <td>{r.adobe === true ? '✅' : r.adobe === false ? '❌' : r.adobe}</td>
                <td>{r.purview === true ? '✅' : r.purview === false ? '❌' : r.purview}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function DemoStrip() {
  const raw =
    'Hi — reaching out about acct 4123-4567-8901-2345.\nSarah Chen, SSN 123-45-6789,\nemail sarah@northside.io, +1 555-123-4567.';
  const clean =
    'Hi — reaching out about acct [CREDIT_CARD].\n[NAME], SSN [SSN],\nemail [EMAIL], [PHONE].';
  const [purified, setPurified] = useState(false);
  return (
    <section className="demo-strip-wrap">
      <div className="demo-strip">
        <div className="demo-pane">
          <h4>What you have</h4>
          <pre>{raw}</pre>
        </div>
        <button className="demo-toggle" onClick={() => setPurified((p) => !p)}>
          {purified ? '⟲ Reset' : 'Purify →'}
        </button>
        <div className="demo-pane purified">
          <h4>What you can share · 0ms to any server</h4>
          <pre>{purified ? clean : raw}</pre>
        </div>
      </div>
    </section>
  );
}

function HowItWorks() {
  const steps = [
    { n: 1, title: 'Download', body: 'Grab the free binary for your OS. No signup.' },
    {
      n: 2,
      title: 'Drop in a file',
      body: 'PDF, Word, Excel, JSON, logs — or paste text. Pick what to redact.',
    },
    {
      n: 3,
      title: 'Get a clean copy',
      body: 'PII is gone — actually deleted, not painted over. Share it anywhere.',
    },
  ];
  return (
    <section className="how-it-works" id="how">
      <h2>Three steps. About a minute.</h2>
      <div className="steps-row">
        {steps.map((s) => (
          <div key={s.n} className="step-card">
            <div className="step-num">{s.n}</div>
            <h3>{s.title}</h3>
            <p>{s.body}</p>
          </div>
        ))}
      </div>
    </section>
  );
}

function Features() {
  const features = [
    {
      icon: '🎯',
      title: 'Catches what you would miss',
      body: 'Emails, SSNs, phones, credit cards (Luhn-checked), routing numbers, addresses, AWS keys, GitHub tokens, JWTs — plus your own custom rules.',
    },
    {
      icon: '✂️',
      title: 'Actually deletes it',
      body: "Not a black box drawn over the text — the PII bytes are removed. For PDFs, the text objects are gone and the page can be flattened to an image.",
    },
    {
      icon: '📄',
      title: 'Every format you have',
      body: 'PDF, DOCX, XLSX, PPTX, EPUB, RTF, Markdown, HTML, JSON, plain text, and live streams.',
    },
    {
      icon: '🚫',
      title: 'Zero cloud. Literally zero.',
      body: 'It runs on your machine. No uploads, no telemetry, no accounts. The daemon binds to localhost only — it cannot be reached from the network.',
    },
  ];
  return (
    <section className="features">
      {features.map((f) => (
        <div key={f.title} className="feature-card">
          <div className="feature-icon">{f.icon}</div>
          <h3>{f.title}</h3>
          <p>{f.body}</p>
        </div>
      ))}
    </section>
  );
}

function Pricing() {
  return (
    <section className="pricing" id="pricing">
      <div className="intro-banner">⏳ Introductory pricing — rates increase as the product matures. Lock in lifetime access now.</div>
      <h2>Free to start. Pay once for lifetime access.</h2>
      <p className="pricing-sub">
        PDF/office formats, name detection, batch processing. Keys are verified
        offline; nothing phones home.
      </p>
      <div className="tiers">
        <div className="tier">
          <h3>Free</h3>
          <div className="price">
            $0<span className="price-sub">/forever</span>
          </div>
          <p className="tier-tag">No key, no account — just run it.</p>
          <ul>
            <li>✅ Text &amp; stream redaction</li>
            <li>✅ Core PII (email, SSN, phone, card, address, secrets)</li>
            <li>✅ Local dashboard</li>
            <li>✅ Mask / placeholder output</li>
          </ul>
        </div>
        <div className="tier tier-cli">
          <div className="tier-badge">Most popular</div>
          <h3>Solo</h3>
          <div className="price">
            <span className="price-strike">$149</span>
            $99<span className="price-sub">once · lifetime</span>
          </div>
          <p className="tier-tag">For one person handling real documents. Pay once, own forever.</p>
          <ul>
            <li>Everything in Free, plus:</li>
            <li>⚡ PDF redaction (delete + flatten)</li>
            <li>⚡ Word, Excel, PowerPoint, EPUB</li>
            <li>⚡ Name detection (your custom list)</li>
            <li>⚡ Batch a whole folder at once</li>
          </ul>
          <a className="btn-primary btn-block" href="#get-started">
            Get a Solo key
          </a>
        </div>
        <div className="tier">
          <h3>Business</h3>
          <div className="price">
            <span className="price-strike">$599</span>
            $399<span className="price-sub">once · lifetime</span>
          </div>
          <p className="tier-tag">For teams that process a lot. 5 seats, one-time.</p>
          <ul>
            <li>Everything in Solo, plus:</li>
            <li>👥 5 seats</li>
            <li>🔧 Custom regex rule builder</li>
            <li>📋 Local audit logging</li>
            <li>🛟 Priority support</li>
          </ul>
          <a className="btn-outline btn-block" href="#get-started">
            Get a Business key
          </a>
        </div>
      </div>
      <div className="get-started-box" id="get-started">
        <h3>Buy a key</h3>
        <p>
          Pay by card via PayPal (the key is emailed to you instantly). AI
          agents can also pay directly with USDC over x402 — see{' '}
          <a href="#agents">For agents</a>.
        </p>
        <ErrorBoundary><Checkout /></ErrorBoundary>
      </div>
    </section>
  );
}

/** The agentic surface — discoverable, well-packaged, pay-per-use. */
function ForAgents() {
  return (
    <section className="for-agents" id="agents">
      <h2>For AI agents</h2>
      <p className="section-sub">
        Agents can purchase licenses programmatically with USDC over the x402
        protocol — no human in the loop, no account to create.
      </p>
      <div className="agent-grid">
        <div className="agent-card">
          <h3>1 · Discover</h3>
          <p>Fetch the machine-readable manifest:</p>
          <pre>
            <code>GET /.well-known/x402.json</code>
          </pre>
        </div>
        <div className="agent-card">
          <h3>2 · Pay (x402)</h3>
          <p>POST without payment → get a 402. Retry with the payment header:</p>
          <pre>
            <code>POST /api/v1/license/x402?tier=solo</code>
          </pre>
        </div>
        <div className="agent-card">
          <h3>3 · Receive</h3>
          <p>A signed <code>BV-SOLO-…</code> key lands in the 200 JSON body. Hand it to the CLI:</p>
          <pre>
            <code>bitvanes config --key BV-SOLO-…</code>
          </pre>
        </div>
      </div>
      <p className="agent-foot">
        Solo <strong>$99 lifetime</strong> · Business <strong>$399 lifetime</strong> · USDC on Base.
        Keys are Ed25519-signed JWTs with a 100-year validity window, verified
        fully offline — your agent never has to trust a server, just the
        embedded public key.
      </p>
    </section>
  );
}

function ZeroTrustCallout() {
  return (
    <section className="zero-trust">
      <h3>Your data never touches our servers. Really.</h3>
      <p>
        BitVanes is air-gapped by design. The engine contains no network code.
        The dashboard is a local UI for a daemon that binds to 127.0.0.1 only.
        We literally cannot see your files — and neither can anyone else.
      </p>
    </section>
  );
}

function Footer() {
  return (
    <footer className="landing-footer">
      <a href="#top" className="logo">
        Bit<span style={{ color: 'var(--accent)' }}>Vanes</span>
      </a>
      <div className="footer-links">
        <a href={REPO}>GitHub</a>
        <a href={RELEASES}>Releases</a>
        <a href={`${REPO}#readme`}>Docs</a>
        <a href="#agents">For Agents</a>
      </div>
      <span>© BitVanes · MIT OR Apache-2.0</span>
    </footer>
  );
}
