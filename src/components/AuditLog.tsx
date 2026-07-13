/**
 * Audit Log Visualizer: renders the PII findings emitted by the engine
 * alongside the original (pre-scrub) document text, so users can see exactly
 * what was detected, where, and with what confidence.
 *
 * Three panels:
 *  1. Findings table — entity, offset range, confidence, anchors (sortable).
 *  2. Highlighted original text — each finding's range is coloured by entity.
 *  3. Per-finding detail — the matched text + anchors that fired.
 */

import { useState, useMemo, type ReactElement } from 'react';
import type { ChunkRow, PiiFinding } from '../lib/wasm-bridge';

interface AuditLogProps {
  chunks: ChunkRow[];
  originalText: string;
}

const ENTITY_COLORS: Record<string, string> = {
  email: '#3b82f6',
  ssn: '#ef4444',
  phone: '#f59e0b',
  credit_card: '#a855f7',
  routing_number: '#ec4899',
  aws_key: '#10b981',
  github_pat: '#14b8a6',
  jwt: '#6366f1',
};

function colorFor(entity: string): string {
  return ENTITY_COLORS[entity] ?? '#6b7280';
}

/** Collects all findings across chunks, deduplicated by offset range. */
function collectFindings(chunks: ChunkRow[]): PiiFinding[] {
  const seen = new Set<string>();
  const all: PiiFinding[] = [];
  for (const c of chunks) {
    for (const f of c.pii) {
      const key = `${f.entity}:${f.offset_start}:${f.offset_end}`;
      if (!seen.has(key)) {
        seen.add(key);
        all.push(f);
      }
    }
  }
  return all.sort((a, b) => a.offset_start - b.offset_start);
}

/**
 * Builds a highlighted representation of the original text by splitting it
 * at finding boundaries and interleaving <mark> spans.
 */
function HighlightedText({ text, findings }: { text: string; findings: PiiFinding[] }): ReactElement {
  const segments = useMemo(() => {
    if (findings.length === 0) return [{ text, entity: null, confidence: 0 }];
    const sorted = [...findings].sort((a, b) => a.offset_start - b.offset_start);
    const parts: { text: string; entity: string | null; confidence: number }[] = [];
    let cursor = 0;
    for (const f of sorted) {
      if (f.offset_start > cursor) {
        parts.push({ text: text.slice(cursor, f.offset_start), entity: null, confidence: 0 });
      }
      const matched = text.slice(f.offset_start, f.offset_end) ?? `[${f.entity}]`;
      parts.push({ text: matched, entity: f.entity, confidence: f.confidence });
      cursor = Math.max(cursor, f.offset_end);
    }
    if (cursor < text.length) {
      parts.push({ text: text.slice(cursor), entity: null, confidence: 0 });
    }
    return parts;
  }, [text, findings]);

  return (
    <pre className="audit-highlighted">
      {segments.map((seg, i) =>
        seg.entity ? (
          <mark
            key={i}
            className="audit-mark"
            style={{
              backgroundColor: colorFor(seg.entity) + '33',
              borderBottom: `2px solid ${colorFor(seg.entity)}`,
              color: 'var(--text)',
            }}
            title={`${seg.entity} · conf ${(seg.confidence * 100).toFixed(0)}%`}
          >
            {seg.text}
          </mark>
        ) : (
          <span key={i}>{seg.text}</span>
        ),
      )}
    </pre>
  );
}

export function AuditLog({ chunks, originalText }: AuditLogProps): ReactElement {
  const [sortBy, setSortBy] = useState<'offset' | 'confidence' | 'entity'>('offset');
  const [minConf, setMinConf] = useState(0);

  const allFindings = useMemo(() => collectFindings(chunks), [chunks]);
  const filtered = useMemo(() => {
    const f = allFindings.filter((x) => x.confidence >= minConf);
    if (sortBy === 'confidence') return [...f].sort((a, b) => b.confidence - a.confidence);
    if (sortBy === 'entity') return [...f].sort((a, b) => a.entity.localeCompare(b.entity));
    return [...f].sort((a, b) => a.offset_start - b.offset_start);
  }, [allFindings, sortBy, minConf]);

  if (allFindings.length === 0) {
    return (
      <div className="audit-empty">
        <p>No PII findings in this document.</p>
        <p className="audit-empty-hint">
          Enable PII patterns (email, SSN, credit card…) in the config bar and re-process to see
          the audit log populate.
        </p>
      </div>
    );
  }

  return (
    <div className="audit-log">
      <div className="audit-controls">
        <label>
          Sort by{' '}
          <select value={sortBy} onChange={(e) => setSortBy(e.target.value as typeof sortBy)}>
            <option value="offset">Offset</option>
            <option value="confidence">Confidence</option>
            <option value="entity">Entity</option>
          </select>
        </label>
        <label>
          Min confidence: {(minConf * 100).toFixed(0)}%
          <input
            type="range"
            min="0"
            max="0.99"
            step="0.05"
            value={minConf}
            onChange={(e) => setMinConf(Number(e.target.value))}
          />
        </label>
        <span className="audit-count">
          {filtered.length} of {allFindings.length} findings shown
        </span>
      </div>

      <div className="audit-grid">
        <div className="audit-panel">
          <h4 className="audit-panel-title">Findings ({filtered.length})</h4>
          <div className="audit-table-wrap">
            <table className="audit-table">
              <thead>
                <tr>
                  <th>Entity</th>
                  <th>Confidence</th>
                  <th>Offset</th>
                  <th>Anchors</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((f, i) => (
                  <tr key={i}>
                    <td>
                      <span
                        className="entity-badge"
                        style={{ backgroundColor: colorFor(f.entity) + '22', color: colorFor(f.entity) }}
                      >
                        {f.entity}
                      </span>
                    </td>
                    <td>
                      <div className="conf-bar">
                        <div
                          className="conf-fill"
                          style={{ width: `${f.confidence * 100}%`, backgroundColor: colorFor(f.entity) }}
                        />
                        <span>{(f.confidence * 100).toFixed(0)}%</span>
                      </div>
                    </td>
                    <td className="col-num">
                      {f.offset_start}–{f.offset_end}
                    </td>
                    <td className="col-anchors">
                      {f.anchors_hit.length > 0 ? f.anchors_hit.join(', ') : '—'}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>

        <div className="audit-panel">
          <h4 className="audit-panel-title">Original text (highlighted)</h4>
          <div className="audit-text-wrap">
            <HighlightedText text={originalText} findings={filtered} />
          </div>
        </div>
      </div>
    </div>
  );
}
