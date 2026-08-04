import { useState, useEffect, useRef, useCallback } from 'react';

/**
 * Interactive in-browser PII scrubbing demo.
 *
 * Uses the bitvanes-wasm `quick_scrub` function — the real engine, compiled to
 * WebAssembly, running entirely in the browser. No daemon, no network. The
 * visitor pastes their own text and watches PII get redacted live.
 */

let wasmReady: Promise<any> | null = null;
async function loadWasm() {
  if (!wasmReady) {
    wasmReady = import('../wasm/bitvanes_wasm.js').then((mod) => mod.default());
  }
  return wasmReady;
}

const SAMPLES = [
  {
    label: 'Email chain',
    text: `From: sarah.chen@northside.io
To: mike+legal@acme-corp.com
Reply to billing@acme-corp.com or call +15551234567.

Customer SSN on file: 123-45-6789.
Card used: 4242 4242 4242 4242, expires 03/28.
AWS key in the logs: AKIAIOSFODNN7EXAMPLE.
JWT: eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c`,
  },
  {
    label: 'Customer record',
    text: `Account: Jane Doe <jane.doe@gmail.com>
SSN: 987-65-4321
Phone: +14155550100
Routing: 021000021
Address: 123 Main Street, Springfield
GitHub PAT: ghp_abcdefghijklmnopqrstuvwxyz0123456789AB`,
  },
];

const RULES = [
  'email',
  'ssn',
  'phone',
  'credit_card',
  'routing_number',
  'street_address',
  'aws_key',
  'github_pat',
  'jwt',
];

export default function LiveDemo() {
  const [text, setText] = useState(SAMPLES[0].text);
  const [result, setResult] = useState<{ redacted: string; findings: any[] } | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    loadWasm()
      .then(() => setReady(true))
      .catch((e) => setError(String(e)));
  }, []);

  const scrub = useCallback(
    async (input: string) => {
      if (!ready || !input.trim()) {
        setResult(null);
        return;
      }
      setLoading(true);
      try {
        const wasm = await loadWasm();
        const out = wasm.quick_scrub(input, RULES);
        setResult(out);
      } catch (e) {
        setError(String(e));
      } finally {
        setLoading(false);
      }
    },
    [ready],
  );

  // debounce on text change
  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => scrub(text), 300);
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [text, scrub]);

  return (
    <section className="live-demo" id="demo">
      <h2>Try it on your own text</h2>
      <p className="section-sub">
        This is the real BitVanes engine — compiled to WebAssembly, running in
        your browser right now. No data leaves this page. Paste anything and
        watch PII get redacted as you type.
      </p>

      {!ready && !error && <p className="demo-loading">Loading engine…</p>}
      {error && <p className="demo-error">Engine failed to load: {error}</p>}

      {ready && (
        <>
          <div className="demo-samples">
            {SAMPLES.map((s) => (
              <button key={s.label} className="demo-sample-btn" onClick={() => setText(s.text)}>
                {s.label}
              </button>
            ))}
            <button
              className="demo-sample-btn demo-clear"
              onClick={() => {
                setText('');
                setResult(null);
              }}
            >
              Clear
            </button>
          </div>

          <div className="live-demo-grid">
            <div className="live-demo-pane">
              <h4>Paste your text</h4>
              <textarea
                value={text}
                onChange={(e) => setText(e.target.value)}
                rows={12}
                placeholder="Paste an email, a log, a customer record…"
              />
            </div>
            <div className="live-demo-pane purified">
              <h4>
                Redacted{' '}
                {result && result.findings.length > 0 && (
                  <span className="demo-count">{result.findings.length} found</span>
                )}
                {loading && <span className="demo-typing">scrubbing…</span>}
              </h4>
              <textarea
                value={result?.redacted || ''}
                readOnly
                rows={12}
                placeholder="Redacted output appears here as you type."
              />
              {result && result.findings.length > 0 && (
                <div className="demo-findings">
                  {Object.entries(
                    result.findings.reduce<Record<string, number>>((acc, f) => {
                      acc[f.entity] = (acc[f.entity] || 0) + 1;
                      return acc;
                    }, {}),
                  ).map(([entity, count]) => (
                    <span key={entity} className="demo-tag">
                      {entity}: {count}
                    </span>
                  ))}
                </div>
              )}
            </div>
          </div>
          <p className="demo-note">
            Running 100% in your browser via WebAssembly · zero network calls
          </p>
        </>
      )}
    </section>
  );
}
