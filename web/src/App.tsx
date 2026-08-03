import { useEffect, useState } from 'react';
import Landing from './components/Landing';

const RELEASES_URL = 'https://github.com/BitVanes/BitVanes/releases';
const DAEMON = 'http://127.0.0.1:8080';

/**
 * BitVanes web app.
 *
 * Two views:
 *  - `landing`: the public marketing page for bitvanes.com.
 *  - `dashboard`: a local-only PII scrubber that POSTs to the BitVanes daemon
 *    running on 127.0.0.1:8080. No data ever leaves the user's machine — the
 *    dashboard talks only to localhost.
 */
export default function App() {
  const [view, setView] = useState<'landing' | 'dashboard'>('landing');
  if (view === 'landing') {
    return <Landing onOpenDashboard={() => setView('dashboard')} />;
  }
  return <Dashboard onBack={() => setView('landing')} />;
}

function Dashboard({ onBack }: { onBack: () => void }) {
  const [input, setInput] = useState(
    'Contact alice@example.com or 555-123-4567 about acct 4123-4567-8901-2345.',
  );
  const [output, setOutput] = useState('');
  const [categories, setCategories] = useState<[string, number][]>([]);
  const [loading, setLoading] = useState(false);
  const [dragging, setDragging] = useState(false);

  // Daemon reachability. Probed on mount + on demand. When down, we show a
  // setup guide instead of a cryptic fetch error.
  const [daemonUp, setDaemonUp] = useState<boolean | null>(null);

  async function probe() {
    setDaemonUp(null);
    try {
      const res = await fetch(`${DAEMON}/health`, { cache: 'no-store' });
      setDaemonUp(res.ok);
    } catch {
      setDaemonUp(false);
    }
  }

  // Silence the unused-warning when the daemon IS up (probe is still wired to
  // the retry button so users can re-check after starting the daemon).
  useEffect(() => {
    let active = true;
    fetch(`${DAEMON}/health`, { cache: 'no-store' })
      .then((r) => active && setDaemonUp(r.ok))
      .catch(() => active && setDaemonUp(false));
    return () => {
      active = false;
    };
  }, []);

  async function scrub(text: string) {
    setLoading(true);
    setOutput('');
    setCategories([]);
    try {
      const res = await fetch(`${DAEMON}/scrub`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ text }),
      });
      if (!res.ok) throw new Error(`daemon responded ${res.status}`);
      const json: { redacted: string; total: number; categories: [string, number][] } =
        await res.json();
      setOutput(json.redacted);
      setCategories(json.categories);
      setDaemonUp(true);
    } catch {
      setDaemonUp(false);
    } finally {
      setLoading(false);
    }
  }

  async function onDrop(e: React.DragEvent) {
    e.preventDefault();
    setDragging(false);
    const file = e.dataTransfer.files?.[0];
    if (!file) return;
    setInput(await file.text());
  }

  const down = daemonUp === false;

  return (
    <div className="dashboard">
      <header className="dashboard-head">
        <button className="link-btn" onClick={onBack}>
          ← Home
        </button>
        <div className="logo">
          Bit<span style={{ color: 'var(--accent)' }}>Vanes</span>
        </div>
        <a className="link-btn" href={RELEASES_URL} target="_blank" rel="noreferrer">
          Download →
        </a>
      </header>

      <p className="dashboard-info">
        Local dashboard — talks only to <code>{DAEMON}</code>. Your data never
        leaves this machine.
      </p>

      {down ? (
        <DaemonGuide onRetry={probe} />
      ) : (
        <div
          className={'dashboard-grid' + (dragging ? ' dragging' : '')}
          onDragOver={(e) => {
            e.preventDefault();
            setDragging(true);
          }}
          onDragLeave={() => setDragging(false)}
          onDrop={onDrop}
        >
          <div className="pane">
            <h3>Raw input · drop a .txt/.md/.json file</h3>
            <textarea value={input} onChange={(e) => setInput(e.target.value)} rows={10} />
          </div>
          <div className="pane">
            <h3>Purified output</h3>
            <textarea value={output} readOnly rows={10} placeholder="Redacted output appears here." />
          </div>
        </div>
      )}

      {!down && (
        <div className="dashboard-actions">
          <button className="btn-primary" onClick={() => scrub(input)} disabled={loading}>
            {loading ? 'Scrubbing…' : 'Scrub locally'}
          </button>
          {categories.length > 0 && (
            <ul className="cat-list">
              {categories.map(([entity, n]) => (
                <li key={entity}>
                  <code>{entity}</code>: {n}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}

/** Shown when the local daemon isn't reachable. Explains the model + setup. */
function DaemonGuide({ onRetry }: { onRetry: () => void }) {
  return (
    <div className="daemon-guide">
      <h2>No local daemon detected</h2>
      <p>
        BitVanes is <strong>local-first by design</strong> — this dashboard is a
        UI for the BitVanes daemon running on <em>your</em> machine, not a hosted
        service. Your data never leaves your computer. Get it running in three
        steps:
      </p>
      <ol>
        <li>
          <strong>Download</strong> the <code>bitvanes</code> binary for your OS
          from{' '}
          <a href={RELEASES_URL} target="_blank" rel="noreferrer">
            GitHub Releases
          </a>
          .
        </li>
        <li>
          <strong>Start the daemon</strong> in a terminal:
          <pre>
            <code>bitvanes daemon --rules email,ssn,credit_card,phone,street_address</code>
          </pre>
        </li>
        <li>
          <strong>Come back here</strong> and scrub. The dashboard connects
          automatically to <code>{DAEMON}</code>.
        </li>
      </ol>
      <div className="daemon-guide-actions">
        <button className="btn-primary" onClick={onRetry}>
          Retry connection
        </button>
        <a className="btn-outline" href={RELEASES_URL} target="_blank" rel="noreferrer">
          Download BitVanes →
        </a>
      </div>
      <p className="daemon-note">
        Building from source?{' '}
        <a
          href="https://github.com/BitVanes/BitVanes#readme"
          target="_blank"
          rel="noreferrer"
        >
          See the README
        </a>{' '}
        — <code>(cd web && npm run build) && (cd cli && cargo build --release --features dashboard)</code>{' '}
        produces a single binary that serves this dashboard.
      </p>
    </div>
  );
}
