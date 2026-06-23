interface LandingProps {
  onOpenTool: () => void;
}

export function Landing({ onOpenTool }: LandingProps) {
  return (
    <div className="landing">
      <nav className="landing-nav">
        <div className="logo">
          Bit<span>Vanes</span>
        </div>
        <div className="links">
          <a href="https://github.com/BitVanes/core" target="_blank" rel="noreferrer">GitHub</a>
          <a href="https://github.com/BitVanes/cli" target="_blank" rel="noreferrer">CLI</a>
          <a href="#" onClick={(e) => { e.preventDefault(); onOpenTool(); }}>Launch Tool →</a>
        </div>
      </nav>

      <section className="landing-hero">
        <h1>
          Zero-trust document chunking<br />
          for <span className="hl">RAG pipelines</span>
        </h1>
        <p>
          Drop a document. Get BPE-accurate chunks with heading ancestry,
          PII scrubbing, and Apache Arrow output — all in your browser.
          No servers. No data leaves your machine.
        </p>
        <button className="cta" onClick={onOpenTool}>
          Launch Tool →
        </button>
      </section>

      <div className="landing-features">
        <div className="feature-card">
          <div className="icon">🔒</div>
          <h3>Zero-Trust</h3>
          <p>
            Documents are parsed, scrubbed, and chunked entirely in your
            browser via WebAssembly. No file ever touches a server.
          </p>
        </div>
        <div className="feature-card">
          <div className="icon">⚡</div>
          <h3>Zero-Copy Arrow</h3>
          <p>
            Output is delivered as Apache Arrow columnar memory via the C
            Data Interface. No JSON serialization on the data path.
          </p>
        </div>
        <div className="feature-card">
          <div className="icon">✂️</div>
          <h3>BPE-Aware Chunking</h3>
          <p>
            Six OpenAI tokenizers (cl100k_base, o200k_base, and more) with
            structural-boundary-aware splitting that respects headings and
            code blocks.
          </p>
        </div>
        <div className="feature-card">
          <div className="icon">🛡️</div>
          <h3>PII Scrubbing</h3>
          <p>
            Redact emails, SSNs, credit cards, API keys, and JWTs before
            tokenization so sensitive data never appears in a chunk.
          </p>
        </div>
        <div className="feature-card">
          <div className="icon">📄</div>
          <h3>Multi-Format</h3>
          <p>
            Markdown, HTML, plain text, and PDF (via PDF.js). Heading
            ancestry is preserved across all formats.
          </p>
        </div>
        <div className="feature-card">
          <div className="icon">🖥️</div>
          <h3>CLI Companion</h3>
          <p>
            Export a pipeline profile from the web tool and replay it
            headlessly with <code>bitvanes-cli</code> for batch processing
            in CI/CD.
          </p>
        </div>
      </div>

      <footer className="landing-footer">
        <a href="https://github.com/BitVanes/core">Engine</a>
        <a href="https://github.com/BitVanes/cli">CLI</a>
        <a href="https://github.com/BitVanes/web">Web</a>
        <span>· MIT OR Apache-2.0</span>
      </footer>
    </div>
  );
}
