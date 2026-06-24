import { useState, useCallback, useRef, useEffect } from 'react';
import { Landing } from './components/Landing';
import {
  ensureInitialized,
  getVersion,
  processDocument,
  type PipelineConfig,
  type ChunkRow,
} from './lib/wasm-bridge';
import {
  exportJSON,
  exportCSV,
  exportJSONL,
  exportArrowIPC,
  exportProfile,
  importProfile,
} from './lib/export';
import { extractPdfText } from './lib/pdf';
import {
  ensureEmbedder,
  embed,
  getEmbeddingDim,
  type EmbeddingStatus,
} from './lib/embeddings';
import {
  syncToVectorDB,
  type VectorDBConfig,
  type SyncProgress,
} from './lib/vector-db';

type ArrowBytes = Uint8Array;

const DEFAULT_CONFIG: PipelineConfig = {
  format: 'markdown',
  scrub: { patterns: ['email'], custom: [] },
  chunk: { max_tokens: 512, overlap_tokens: 0, tokenizer: 'cl100k_base' },
};

const FORMATS = ['markdown', 'text', 'html', 'json'] as const;
const TOKENIZERS = ['cl100k_base', 'o200k_base', 'r50k_base', 'p50k_base', 'p50k_edit', 'o200k_harmony'] as const;
const PII_PATTERNS = ['email', 'ssn', 'phone', 'credit_card', 'aws_key', 'github_pat', 'jwt'] as const;

export default function App() {
  const [view, setView] = useState<'landing' | 'tool'>('landing');

  if (view === 'landing') {
    return <Landing onOpenTool={() => setView('tool')} />;
  }
  return <Tool onBack={() => setView('landing')} />;
}

function Tool({ onBack }: { onBack: () => void }) {
  const [initState, setInitState] = useState<'loading' | 'ready' | 'error'>('loading');
  const [errorMsg, setErrorMsg] = useState('');
  const [loading, setLoading] = useState(false);
  const [config, setConfig] = useState<PipelineConfig>(DEFAULT_CONFIG);
  const [chunks, setChunks] = useState<ChunkRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [fileName, setFileName] = useState('');
  const fileInputRef = useRef<HTMLInputElement>(null);
  const profileInputRef = useRef<HTMLInputElement>(null);
  const tableRef = useRef<ArrowBytes | null>(null);

  const [embedStatus, setEmbedStatus] = useState<EmbeddingStatus>({ phase: 'idle' });
  const [embeddings, setEmbeddings] = useState<number[][] | null>(null);

  const [dbConfig, setDbConfig] = useState<VectorDBConfig>({
    provider: 'qdrant', endpoint: '', apiKey: '', collection: 'bitvanes',
  });
  const [syncProgress, setSyncProgress] = useState<SyncProgress | null>(null);
  const [syncing, setSyncing] = useState(false);

  // Auto-initialize wasm on mount.
  useEffect(() => {
    ensureInitialized()
      .then(() => setInitState('ready'))
      .catch((e) => { setErrorMsg(String(e)); setInitState('error'); });
  }, []);

  const handleImportProfile = useCallback(async (file: File) => {
    try {
      const imported = await importProfile(file);
      setConfig(imported);
      setError(null);
    } catch (e) {
      setError(`Could not import profile: ${e}`);
    }
  }, []);

  const updateCustom = useCallback((i: number, patch: Partial<{ regex: string; replacement: string }>) => {
    setConfig((cfg) => {
      const custom = cfg.scrub.custom.map((c, idx) => (idx === i ? { ...c, ...patch } : c));
      return { ...cfg, scrub: { ...cfg.scrub, custom } };
    });
  }, []);

  const addCustom = useCallback(() => {
    setConfig((cfg) => ({ ...cfg, scrub: { ...cfg.scrub, custom: [...cfg.scrub.custom, { regex: '', replacement: '' }] } }));
  }, []);

  const removeCustom = useCallback((i: number) => {
    setConfig((cfg) => ({ ...cfg, scrub: { ...cfg.scrub, custom: cfg.scrub.custom.filter((_, idx) => idx !== i) } }));
  }, []);

  const handleFile = useCallback(
    async (file: File) => {
      setLoading(true);
      setError(null);
      setFileName(file.name);
      tableRef.current = null;
      setChunks([]);
      setEmbeddings(null);
      setEmbedStatus({ phase: 'idle' });
      setSyncProgress(null);

      try {
        const fileBytes = new Uint8Array(await file.arrayBuffer());
        const isPdf = fileBytes.length >= 4 && fileBytes[0] === 0x25 && fileBytes[1] === 0x50 && fileBytes[2] === 0x44 && fileBytes[3] === 0x46;

        let engineBytes: Uint8Array;
        let engineFormat: string;

        if (isPdf) {
          const markdown = await extractPdfText(fileBytes);
          if (!markdown.trim()) throw new Error('PDF.js could not extract text. It may be a scanned image PDF.');
          engineBytes = new TextEncoder().encode(markdown);
          engineFormat = 'markdown';
        } else {
          if (fileBytes.length >= 2 && fileBytes[0] === 0x50 && fileBytes[1] === 0x4b)
            throw new Error('Office documents not supported. Use .pdf, .md, .txt, .html, or .json.');
          try { new TextDecoder('utf-8', { fatal: true }).decode(fileBytes.slice(0, 4096)); }
          catch { throw new Error(`"${file.name}" is not a text file. Supported: .pdf, .md, .txt, .html, .json`); }
          engineBytes = fileBytes;
          engineFormat = config.format;
        }

        const cfg: PipelineConfig = { ...config, format: engineFormat as PipelineConfig['format'], source_label: file.name };
        const result = await processDocument(cfg, engineBytes);
        setChunks(result.chunks);
        tableRef.current = result.arrowBytes;
      } catch (e) {
        setError(String(e));
        setChunks([]);
      } finally {
        setLoading(false);
      }
    },
    [config],
  );

  const handleGenerateEmbeddings = useCallback(async () => {
    setError(null);
    try {
      await ensureEmbedder((s) => setEmbedStatus(s));
      setEmbedStatus({ phase: 'embedding', done: 0, total: chunks.length });
      const result = await embed(chunks.map((c) => c.text), (done, total) => setEmbedStatus({ phase: 'embedding', done, total }));
      setEmbeddings(result);
      setEmbedStatus({ phase: 'ready' });
    } catch (e) {
      setEmbedStatus({ phase: 'error', message: String(e) });
      setError(`Embedding error: ${e}`);
    }
  }, [chunks]);

  const handleSync = useCallback(async () => {
    if (!embeddings) { setError('Generate embeddings first'); return; }
    setSyncing(true); setError(null);
    setSyncProgress({ synced: 0, total: chunks.length, failed: 0 });
    try {
      const { synced, failed } = await syncToVectorDB(
        chunks.map((c) => ({ chunk_index: c.chunk_index, text: c.text, token_count: c.token_count, heading_path: c.heading_path, section_kind: c.section_kind, source_path: c.source_path })),
        embeddings, dbConfig, (p) => setSyncProgress(p),
      );
      setSyncProgress({
        synced,
        total: chunks.length,
        failed,
        message: `Done! ${synced} chunks synced${failed > 0 ? `, ${failed} failed` : ''}.`,
      });
    } catch (e) { setError(`Sync error: ${e}`); }
    finally { setSyncing(false); }
  }, [chunks, embeddings, dbConfig]);

  const totalTokens = chunks.reduce((sum, c) => sum + c.token_count, 0);

  /** Builds a bucketed histogram of per-chunk token counts. */
  const histogram = (() => {
    if (chunks.length === 0) return null;
    const counts = chunks.map((c) => c.token_count);
    const max = Math.max(...counts, 1);
    const buckets = 8;
    const bins = new Array(buckets).fill(0);
    for (const t of counts) {
      const idx = Math.min(buckets - 1, Math.floor((t / max) * buckets));
      bins[idx]++;
    }
    const maxBin = Math.max(...bins, 1);
    return { bins, max, maxBin, buckets };
  })();

  // Loading state while wasm initializes.
  if (initState === 'loading') {
    return (
      <div className="init-screen">
        <div className="spinner" />
        <p style={{ marginTop: 16, color: 'var(--text-muted)', fontSize: 14 }}>Loading engine…</p>
      </div>
    );
  }

  if (initState === 'error') {
    return (
      <div className="init-screen">
        <div className="error-msg" style={{ maxWidth: 400 }}>{errorMsg}</div>
        <button className="cta" style={{ marginTop: 16 }} onClick={onBack}>← Back</button>
      </div>
    );
  }

  return (
    <div className="tool-page">
      <header className="tool-header">
        <div className="tool-header-left">
          <button className="back-link" onClick={onBack}>← Home</button>
          <span className="logo-small">Bit<span style={{ color: 'var(--accent)' }}>Vanes</span></span>
        </div>
        <a className="cli-link" href="https://github.com/BitVanes/cli" target="_blank" rel="noreferrer">CLI →</a>
      </header>

      <div className="tool-body">
        <div className="tool-info">
          Drop a file below. Configure chunking, generate embeddings, export, or sync to a vector DB —
          all in your browser. Export a profile to replay with <code>bitvanes-cli</code>.
        </div>

        <div className="config-bar">
          <label>Format
            <select value={config.format} onChange={(e) => setConfig({ ...config, format: e.target.value as PipelineConfig['format'] })}>
              {FORMATS.map((f) => <option key={f} value={f}>{f}</option>)}
            </select>
          </label>
          <label>Tokenizer
            <select value={config.chunk.tokenizer} onChange={(e) => setConfig({ ...config, chunk: { ...config.chunk, tokenizer: e.target.value } })}>
              {TOKENIZERS.map((t) => <option key={t} value={t}>{t}</option>)}
            </select>
          </label>
          <label>Max Tokens: {config.chunk.max_tokens}
            <input type="range" min="64" max="2048" step="64" value={config.chunk.max_tokens}
              onChange={(e) => setConfig({ ...config, chunk: { ...config.chunk, max_tokens: Number(e.target.value) } })} />
          </label>
          <label>Overlap: {config.chunk.overlap_tokens}
            <input type="range" min="0" max="256" step="16" value={config.chunk.overlap_tokens}
              onChange={(e) => setConfig({ ...config, chunk: { ...config.chunk, overlap_tokens: Number(e.target.value) } })} />
          </label>
          <fieldset className="fieldset">
            <legend>PII Scrubbing</legend>
            {PII_PATTERNS.map((p) => (
              <label key={p} className="checkbox-label">
                <input type="checkbox" checked={config.scrub.patterns.includes(p)}
                  onChange={(e) => {
                    const patterns = e.target.checked ? [...config.scrub.patterns, p] : config.scrub.patterns.filter((x) => x !== p);
                    setConfig({ ...config, scrub: { ...config.scrub, patterns } });
                  }} />
                {p}
              </label>
            ))}
            <div className="custom-regex">
              <span className="custom-regex-title">Custom regex</span>
              {config.scrub.custom.map((c, i) => (
                <div className="custom-regex-row" key={i}>
                  <input placeholder="regex (e.g. \\bPROJ-\\d+\\b)" value={c.regex}
                    onChange={(e) => updateCustom(i, { regex: e.target.value })} />
                  <input placeholder="replacement (e.g. [PROJ])" value={c.replacement}
                    onChange={(e) => updateCustom(i, { replacement: e.target.value })} />
                  <button className="btn-outline btn-tiny" onClick={() => removeCustom(i)}>✕</button>
                </div>
              ))}
              <button className="btn-outline btn-tiny" onClick={addCustom}>+ add pattern</button>
            </div>
          </fieldset>
        </div>

        <div className="dropzone" onClick={() => fileInputRef.current?.click()}
          onDrop={(e) => { e.preventDefault(); const file = e.dataTransfer.files[0]; if (file) handleFile(file); }}
          onDragOver={(e) => e.preventDefault()}>
          <input ref={fileInputRef} type="file" style={{ display: 'none' }}
            onChange={(e) => { const file = e.target.files?.[0]; if (file) handleFile(file); }} />
          {loading ? <span className="dropzone-text">Processing…</span>
            : fileName ? <span className="dropzone-text">{fileName} — drop another to reprocess</span>
            : <span className="dropzone-text">Drop a .pdf / .md / .txt / .html file here</span>}
        </div>

        {error && <div className="error-msg">{error}</div>}

        {chunks.length > 0 && (
          <div className="results-section">
            <div className="stats">
              <strong>{chunks.length}</strong> chunks · <strong>{totalTokens}</strong> tokens · <strong>{(totalTokens / chunks.length).toFixed(0)}</strong> avg
            </div>

            <div className="export-bar">
              <button className="btn-outline" onClick={() => exportJSON(chunks, fileName)}>JSON</button>
              <button className="btn-outline" onClick={() => exportCSV(chunks, fileName)}>CSV</button>
              <button className="btn-outline" disabled={!tableRef.current} onClick={() => tableRef.current && exportArrowIPC(tableRef.current, fileName)}>Arrow IPC</button>
              <button className="btn-outline" disabled={!embeddings} onClick={() => embeddings && exportJSONL(chunks, embeddings, fileName)}>JSONL + embeddings</button>
              <button className="btn-outline" onClick={() => exportProfile(config, fileName)}>Export Profile</button>
              <button className="btn-outline" onClick={() => profileInputRef.current?.click()}>Import Profile</button>
              <input ref={profileInputRef} type="file" accept=".json,application/json" style={{ display: 'none' }}
                onChange={(e) => { const f = e.target.files?.[0]; if (f) handleImportProfile(f); e.target.value = ''; }} />
            </div>

            {embedStatus.phase === 'idle' && (
              <button className="btn-primary" onClick={handleGenerateEmbeddings} disabled={chunks.length === 0}>
                Generate Embeddings ({getEmbeddingDim()}-dim)
              </button>
            )}
            {embedStatus.phase === 'downloading' && (
              <div className="progress-section">
                <p className="progress-text">{embedStatus.detail}</p>
                <div className="progress-bar"><div className="progress-fill" style={{ width: `${embedStatus.progress}%` }} /></div>
              </div>
            )}
            {embedStatus.phase === 'embedding' && (
              <div className="progress-section">
                <p className="progress-text">Generating: {embedStatus.done}/{embedStatus.total}</p>
                <div className="progress-bar"><div className="progress-fill" style={{ width: `${(embedStatus.done / embedStatus.total) * 100}%` }} /></div>
              </div>
            )}
            {embedStatus.phase === 'ready' && <p className="success-text">✓ {embeddings?.length ?? 0} embeddings ready ({getEmbeddingDim()}-dim)</p>}

            {embeddings && (
              <div className="sync-panel">
                <div className="sync-config">
                  <select value={dbConfig.provider} onChange={(e) => setDbConfig({ ...dbConfig, provider: e.target.value as 'qdrant' | 'pinecone' })}>
                    <option value="qdrant">Qdrant</option>
                    <option value="pinecone">Pinecone</option>
                  </select>
                  <input placeholder={dbConfig.provider === 'qdrant' ? 'https://localhost:6333' : 'https://index-xxx.svc.pinecone.io'}
                    value={dbConfig.endpoint} onChange={(e) => setDbConfig({ ...dbConfig, endpoint: e.target.value })} />
                  <input placeholder="API key" type="password"
                    value={dbConfig.apiKey} onChange={(e) => setDbConfig({ ...dbConfig, apiKey: e.target.value })} />
                  <input placeholder="collection" value={dbConfig.collection}
                    onChange={(e) => setDbConfig({ ...dbConfig, collection: e.target.value })} />
                  <button className="btn-primary btn-small" onClick={handleSync} disabled={syncing || !dbConfig.endpoint}>
                    {syncing ? 'Syncing…' : `Sync ${chunks.length} →`}
                  </button>
                </div>
                {syncProgress && (
                  <div className="progress-section">
                    <div className="progress-bar"><div className="progress-fill" style={{ width: `${(syncProgress.synced / syncProgress.total) * 100}%` }} /></div>
                    <p className="progress-text">{syncProgress.message ?? `${syncProgress.synced}/${syncProgress.total} synced${syncProgress.failed > 0 ? `, ${syncProgress.failed} failed` : ''}`}</p>
                  </div>
                )}
              </div>
            )}

            <div className="table-wrap">
              <table>
                <thead><tr><th>#</th><th>Text</th><th>Tokens</th><th>Heading</th><th>Kind</th></tr></thead>
                <tbody>
                  {chunks.slice(0, 200).map((c) => (
                    <tr key={c.chunk_index}>
                      <td className="col-idx">{c.chunk_index}</td>
                      <td className="col-text">{c.text.length > 120 ? c.text.slice(0, 120) + '…' : c.text}</td>
                      <td className="col-num">{c.token_count}</td>
                      <td className="col-heading">{c.heading_path.length > 0 ? c.heading_path.join(' › ') : '—'}</td>
                      <td className="col-kind">{c.section_kind}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {chunks.length > 200 && <p className="truncated">Showing first 200 of {chunks.length}</p>}
            </div>

            {histogram && (
              <div className="histogram">
                <div className="histogram-title">Token distribution</div>
                <div className="histogram-bars">
                  {histogram.bins.map((n, i) => {
                    const lo = Math.round((i / histogram.buckets) * histogram.max);
                    const hi = Math.round(((i + 1) / histogram.buckets) * histogram.max);
                    return (
                      <div className="histogram-col" key={i} title={`${lo}–${hi} tokens: ${n} chunks`}>
                        <div className="histogram-bar" style={{ height: `${(n / histogram.maxBin) * 100}%` }} />
                        <span className="histogram-count">{n}</span>
                      </div>
                    );
                  })}
                </div>
              </div>
            )}
          </div>
        )}

        <footer className="engine-version">engine v{getVersion()}</footer>
      </div>
    </div>
  );
}
