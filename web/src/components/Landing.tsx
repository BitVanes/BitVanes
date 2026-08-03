import { useState } from 'react';

/**
 * Public landing page for bitvanes.com.
 */
export default function Landing({ onOpenDashboard }: { onOpenDashboard: () => void }) {
  return (
    <div className="landing">
      <Nav onOpenDashboard={onOpenDashboard} />
      <Hero onOpenDashboard={onOpenDashboard} />
      <DemoStrip />
      <Features />
      <GetStarted onOpenDashboard={onOpenDashboard} />
      <Pricing />
      <ZeroTrustCallout />
      <Footer />
    </div>
  );
}

const REPO = 'https://github.com/BitVanes/BitVanes';
const RELEASES = 'https://github.com/BitVanes/BitVanes/releases';

function Nav({ onOpenDashboard }: { onOpenDashboard: () => void }) {
  return (
    <nav className="landing-nav">
      <div className="logo">
        Bit<span style={{ color: 'var(--accent)' }}>Vanes</span>
      </div>
      <div className="nav-links">
        <a href={REPO} target="_blank" rel="noreferrer">
          GitHub
        </a>
        <a href={RELEASES} target="_blank" rel="noreferrer">
          Download
        </a>
        <button className="link-btn" onClick={onOpenDashboard}>
          Launch Dashboard →
        </button>
      </div>
    </nav>
  );
}

function Hero({ onOpenDashboard }: { onOpenDashboard: () => void }) {
  return (
    <header className="landing-hero">
      <h1>
        Scrub Sensitive PII in Seconds.
        <br />
        <span className="hl">100% On-Premise. Zero Cloud Leaks.</span>
      </h1>
      <p className="subhead">
        BitVanes directs, filters, and purifies document streams before they reach
        downstream databases, cloud storage, or AI tools. Save 5+ hours of manual
        document scrubbing every week — process 1,000 PDFs in seconds, locally on
        your own hardware.
      </p>
      <div className="hero-ctas">
        <a className="btn-primary" href={RELEASES}>
          Download BitVanes
        </a>
        <button className="btn-outline" onClick={onOpenDashboard}>
          Open Local Dashboard
        </button>
      </div>
    </header>
  );
}

/** Animated side-by-side: raw stream vs purified stream. */
function DemoStrip() {
  const raw =
    'Contact alice@example.com or 555-123-4567.\nSSN 123-45-6789 on file.\nCard 4123 4567 8901 2345.';
  const clean =
    'Contact [REDACTED_EMAIL] or [REDACTED_PHONE].\n[REDACTED_SSN] on file.\n[REDACTED_CREDIT_CARD].';
  const [purified, setPurified] = useState(false);
  return (
    <section className="demo-strip">
      <div className="demo-pane">
        <h4>Raw stream</h4>
        <pre>{raw}</pre>
      </div>
      <button className="demo-toggle" onClick={() => setPurified((p) => !p)}>
        {purified ? '⟲ Reset' : 'Purify →'}
      </button>
      <div className="demo-pane purified">
        <h4>Purified stream · 0ms to any external server</h4>
        <pre>{purified ? clean : raw}</pre>
      </div>
    </section>
  );
}

function Features() {
  return (
    <section className="features">
      <Feature
        icon="🔒"
        title="Air-Gapped Local Redaction"
        body="Everything runs on your hardware. No cloud uploads, no telemetry, no compliance risk. The engine makes zero network calls."
      />
      <Feature
        icon="⚡"
        title="Deterministic Stream Filtering"
        body="Regex + Luhn/ABA-validated detection with confidence scoring. Pipe a stream through bitvanes filter or scrub a directory of 1,000 documents in seconds."
      />
      <Feature
        icon="🛡️"
        title="Zero-Trust Data Cleansing"
        body="Mask, hash, or redact PII (email, SSN, phone, credit card, routing, addresses, API keys, JWTs). Configurable rules via Bitvanes.toml."
      />
      <Feature
        icon="📄"
        title="Local Document Sanitization"
        body="PDF, DOCX, PPTX, XLSX, EPUB, RTF, Markdown, HTML, JSON, and plain text — parsed and sanitized before they reach downstream systems."
      />
    </section>
  );
}

function Feature({ icon, title, body }: { icon: string; title: string; body: string }) {
  return (
    <div className="feature-card">
      <div className="feature-icon">{icon}</div>
      <h3>{title}</h3>
      <p>{body}</p>
    </div>
  );
}

function GetStarted({ onOpenDashboard }: { onOpenDashboard: () => void }) {
  return (
    <section className="get-started" id="get-started">
      <h2>Run it in 60 seconds</h2>
      <p className="pricing-sub">
        BitVanes is local-first — no account, no cloud. Download the binary and
        start the daemon.
      </p>
      <div className="steps">
        <div className="step">
          <div className="step-num">1</div>
          <h4>Download</h4>
          <p>
            Grab the <code>bitvanes</code> binary for your OS from{' '}
            <a href={RELEASES} target="_blank" rel="noreferrer">
              GitHub Releases
            </a>{' '}
            (or{' '}
            <a href={REPO} target="_blank" rel="noreferrer">
              build from source
            </a>
            ).
          </p>
        </div>
        <div className="step">
          <div className="step-num">2</div>
          <h4>Start the daemon</h4>
          <pre>
            <code>bitvanes daemon --rules email,ssn,credit_card,phone,street_address</code>
          </pre>
        </div>
        <div className="step">
          <div className="step-num">3</div>
          <h4>Scrub</h4>
          <p>
            Open the local dashboard and drop in a file, or pipe a stream:{' '}
            <code>cat f.json | bitvanes filter</code>.
          </p>
          <button className="btn-outline" onClick={onOpenDashboard}>
            Open Local Dashboard →
          </button>
        </div>
      </div>
    </section>
  );
}

function Pricing() {
  return (
    <section className="pricing">
      <h2>Pricing</h2>
      <p className="pricing-sub">
        Free for core text scrubbing. One-time or annual keys unlock document +
        automation features. No cloud, no telemetry — your license key unlocks
        local features offline.
      </p>
      <div className="tiers">
        <div className="tier">
          <h3>Free</h3>
          <div className="price">
            $0<span className="price-sub">/forever</span>
          </div>
          <p className="tier-tag">No key needed — just run it.</p>
          <ul>
            <li>Text &amp; stream scrubbing (<code>scrub</code> / <code>filter</code>)</li>
            <li>Core PII: email, SSN, phone, card, routing, address, secrets</li>
            <li>Local daemon + this dashboard</li>
            <li>Mask / placeholder redaction</li>
          </ul>
        </div>
        <div className="tier tier-cli">
          <h3>Solo</h3>
          <div className="price">
            $12<span className="price-sub">/mo · $99/yr</span>
          </div>
          <p className="tier-tag">1 seat · unlocks Pro features.</p>
          <ul>
            <li>PDF destructive redaction (<code>--pdf-mode redact</code>)</li>
            <li>Office formats (DOCX, XLSX, PPTX, EPUB, RTF)</li>
            <li>Personal-name gazetteer</li>
            <li>Batch / directory processing</li>
            <li>Hash redaction policy</li>
          </ul>
          <a className="btn-primary" href="#get-started">
            Buy a key
          </a>
        </div>
        <div className="tier">
          <h3>Business</h3>
          <div className="price">
            $49<span className="price-sub">/mo · $399/yr</span>
          </div>
          <p className="tier-tag">5 seats · for teams.</p>
          <ul>
            <li>Everything in Solo</li>
            <li>5 license seats</li>
            <li>Custom regex rule builder</li>
            <li>Local audit logging</li>
            <li>Priority support</li>
          </ul>
          <a className="btn-outline" href="#get-started">
            Buy a key
          </a>
        </div>
      </div>
    </section>
  );
}

function ZeroTrustCallout() {
  return (
    <section className="zero-trust">
      <h3>Zero-Trust Guarantee</h3>
      <p>
        Your data never touches our servers. BitVanes operates completely air-gapped
        on your local hardware — the engine contains no network code, and the daemon
        binds to loopback (127.0.0.1) only.
      </p>
    </section>
  );
}

function Footer() {
  return (
    <footer className="landing-footer">
      <div className="logo">
        Bit<span style={{ color: 'var(--accent)' }}>Vanes</span>
      </div>
      <div className="footer-links">
        <a href={REPO}>GitHub</a>
        <a href={RELEASES}>Releases</a>
        <a href={`${REPO}#readme`}>Docs</a>
      </div>
      <span>· MIT OR Apache-2.0</span>
    </footer>
  );
}
