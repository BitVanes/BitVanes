import { useState } from 'react';
import Landing from './components/Landing';

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

/** Default daemon origin. Loopback only. */
const DAEMON = 'http://127.0.0.1:8080';

function Dashboard({ onBack }: { onBack: () => void }) {
  const [input, setInput] = useState(
    'Contact alice@example.com or 555-123-4567 about acct 4123-4567-8901-2345.',
  );
  const [output, setOutput] = useState('');
  const [categories, setCategories] = useState<[string, number][]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [dragging, setDragging] = useState(false);

  async function scrub(text: string) {
    setError(null);
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
    } catch (e) {
      setError(
        `Could not reach the local daemon at ${DAEMON}. Start it with ` +
          `\`bitvanes daemon\` (built with the dashboard feature). (${String(e)})`,
      );
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

  return (
    <div className="dashboard">
      <header className="dashboard-head">
        <button className="link-btn" onClick={onBack}>
          ← Home
        </button>
        <div className="logo">
          Bit<span style={{ color: 'var(--accent)' }}>Vanes</span>
        </div>
        <a
          className="link-btn"
          href="https://github.com/BitVanes/cli"
          target="_blank"
          rel="noreferrer"
        >
          CLI →
        </a>
      </header>

      <p className="dashboard-info">
        Local dashboard — talks only to <code>{DAEMON}</code>. Your data never
        leaves this machine.
      </p>

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

      {error && <p className="error-text">{error}</p>}
    </div>
  );
}
