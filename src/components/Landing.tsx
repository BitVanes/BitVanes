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
      <Pricing />
      <ZeroTrustCallout />
      <Footer />
    </div>
  );
}

function Nav({ onOpenDashboard }: { onOpenDashboard: () => void }) {
  return (
    <nav className="landing-nav">
      <div className="logo">
        Bit<span style={{ color: 'var(--accent)' }}>Vanes</span>
      </div>
      <div className="nav-links">
        <a href="https://github.com/BitVanes/core" target="_blank" rel="noreferrer">
          Engine
        </a>
        <a href="https://github.com/BitVanes/cli" target="_blank" rel="noreferrer">
          CLI
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
        <a className="btn-primary" href="https://github.com/BitVanes/cli/releases">
          Download CLI / Desktop App
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

function Pricing() {
  return (
    <section className="pricing">
      <h2>Pricing</h2>
      <p className="pricing-sub">Utility pricing for compliance, legal, and ops teams.</p>
      <div className="tiers">
        <div className="tier">
          <h3>Free / Community</h3>
          <div className="price">
            $0<span className="price-sub">/mo</span>
          </div>
          <ul>
            <li>CLI &amp; local daemon</li>
            <li>Up to 10 files per batch</li>
            <li>Standard PII ruleset (SSN, email, phone, credit cards)</li>
            <li>Great for dev evaluation</li>
          </ul>
        </div>
        <div className="tier tier-cli">
          <h3>Pro / Solo</h3>
          <div className="price">
            $12<span className="price-sub">/mo</span>
          </div>
          <ul>
            <li>1 license seat</li>
            <li>Unlimited local processing</li>
            <li>Standard PII ruleset (SSN, email, phone, credit cards)</li>
            <li>CLI + local dashboard</li>
          </ul>
        </div>
        <div className="tier">
          <h3>Business</h3>
          <div className="price">
            $49<span className="price-sub">/mo</span>
          </div>
          <ul>
            <li>5 license seats</li>
            <li>Custom regex rule builder</li>
            <li>Local audit logging</li>
            <li>Priority CLI support</li>
          </ul>
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
        <a href="https://github.com/BitVanes/core">Engine</a>
        <a href="https://github.com/BitVanes/cli">CLI</a>
        <a href="https://github.com/BitVanes/web">Web</a>
      </div>
      <span>· MIT OR Apache-2.0</span>
    </footer>
  );
}
