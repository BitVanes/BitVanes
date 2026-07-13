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
          confidence-scored PII scrubbing, and Apache Arrow output — all in
          your browser. No servers. No data leaves your machine.
        </p>
        <div className="hero-ctas">
          <button className="cta" onClick={onOpenTool}>
            Try the Playground →
          </button>
          <a className="cta-secondary" href="https://github.com/BitVanes/cli" target="_blank" rel="noreferrer">
            Get the CLI for production
          </a>
        </div>
      </section>

      <div className="landing-features">
        <div className="feature-card">
          <div className="icon">🔒</div>
          <h3>Zero-Trust</h3>
          <p>
            Documents are parsed, scrubbed, and chunked entirely client-side
            via WebAssembly. No file ever touches a server.
          </p>
        </div>
        <div className="feature-card">
          <div className="icon">⚡</div>
          <h3>Zero-Copy Arrow</h3>
          <p>
            Output is Apache Arrow columnar memory via the C Data Interface.
            11 columns including <code>chunk_id</code>, <code>pii_metadata</code>,
            and <code>embedding</code>.
          </p>
        </div>
        <div className="feature-card">
          <div className="icon">🛡️</div>
          <h3>Confidence-Scored PII</h3>
          <p>
            Weighted-additive scoring with contextual anchor windows. Luhn/ABA
            checksums gate credit cards and routing numbers. Every finding
            carries an explainable confidence and the anchors that fired.
          </p>
        </div>
        <div className="feature-card">
          <div className="icon">✂️</div>
          <h3>BPE-Aware Chunking</h3>
          <p>
            Six OpenAI tokenizers with structural-boundary-aware splitting
            that respects headings, code blocks, and tables.
          </p>
        </div>
      </div>

      <section className="tier-comparison">
        <h2>Web Playground vs. CLI</h2>
        <p className="tier-subtitle">
          The browser sandbox is great for evaluation. For production ETL at scale,
          use the CLI — it's the fastest path.
        </p>
        <div className="tier-grid">
          <div className="tier-col tier-web">
            <h3>Web (this page)</h3>
            <ul>
              <li>✅ Zero-trust sandbox — no data leaves browser</li>
              <li>✅ Drag-and-drop playground with audit log</li>
              <li>✅ SIMD-enabled wasm, off-main-thread worker</li>
              <li className="tier-limit">⚠ Single-threaded (no rayon)</li>
              <li className="tier-limit">⚠ ~2 GB memory cap</li>
              <li className="tier-limit">⚠ Markdown / HTML / Text / JSON / PDF only</li>
              <li className="tier-limit">⚠ No DOCX / PPTX / XLSX / EPUB / RTF</li>
              <li className="tier-limit">⚠ No streaming output</li>
            </ul>
          </div>
          <div className="tier-col tier-cli">
            <h3>CLI (native)</h3>
            <ul>
              <li>✅ <strong>All formats</strong>: MD, HTML, TXT, JSON, PDF, DOCX, PPTX, XLSX, EPUB, RTF</li>
              <li>✅ Rayon parallel batch + intra-doc parallel regex</li>
              <li>✅ Memory-mapped file I/O for large documents</li>
              <li>✅ Streaming Arrow IPC output to stdout or file</li>
              <li>✅ On-device embeddings via ONNX Runtime</li>
              <li>✅ Idempotent manifests, watch mode, glob patterns</li>
              <li>✅ Profile replay: export config here, run headlessly</li>
            </ul>
            <a className="cta" href="https://github.com/BitVanes/cli" target="_blank" rel="noreferrer">
              Install CLI →
            </a>
          </div>
        </div>
      </section>

      <footer className="landing-footer">
        <a href="https://github.com/BitVanes/core">Engine</a>
        <a href="https://github.com/BitVanes/cli">CLI</a>
        <a href="https://github.com/BitVanes/web">Web</a>
        <span>· MIT OR Apache-2.0</span>
      </footer>
    </div>
  );
}
