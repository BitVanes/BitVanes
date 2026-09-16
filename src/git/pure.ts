export interface DiffHunk {
  oldStart: number;
  oldCount: number;
  newStart: number;
  newCount: number;
}

export interface DiffFile {
  path: string;
  absPath: string;
  previousPath?: string;
  status: 'A' | 'M' | 'D' | 'R' | 'T' | 'U';
  hunks: DiffHunk[];
}

const HUNK_RE = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/;

/**
 * Decode a path as emitted by git: strips the trailing TAB that git appends to
 * `+++`/`---` lines for space-containing paths, and unquotes the C-style
 * quoted form (`"b/\303\251t\303\251.rs"`) git uses for non-ASCII/special paths.
 * Octal escapes are UTF-8 bytes and are decoded as sequences.
 */
export function decodeGitPath(raw: string): string {
  let s = raw.replace(/\t$/, '');
  if (s.length >= 2 && s.startsWith('"') && s.endsWith('"')) {
    const inner = s.slice(1, -1);
    const bytes: number[] = [];
    let out = '';
    const decoder = new TextDecoder('utf-8', { fatal: false });
    const flush = (): void => {
      if (bytes.length > 0) {
        out += decoder.decode(Uint8Array.from(bytes));
        bytes.length = 0;
      }
    };
    for (let i = 0; i < inner.length; i++) {
      const m = /^\\([0-7]{1,3})/.exec(inner.slice(i));
      if (m && m[1] !== undefined) {
        bytes.push(parseInt(m[1], 8));
        i += m[0].length - 1;
        continue;
      }
      if (inner[i] === '\\' && i + 1 < inner.length) {
        flush();
        const c = inner[i + 1]!;
        out += c === 'n' ? '\n' : c === 't' ? '\t' : c;
        i += 1;
        continue;
      }
      flush();
      out += inner[i];
    }
    flush();
    s = out;
  }
  return s;
}

export function parseUnifiedDiff(text: string): DiffFile[] {
  const files: DiffFile[] = [];
  let current: DiffFile | null = null;

  for (const line of text.split('\n')) {
    const diffGit = /^diff --git (?:"?a\/(.+?)"?) (?:"?b\/(.+?)"?)$/.exec(line);
    if (diffGit && diffGit[1] && diffGit[2]) {
      current = {
        path: decodeGitPath(diffGit[2]),
        absPath: decodeGitPath(diffGit[2]),
        status: 'M',
        hunks: [],
      };
      files.push(current);
      continue;
    }
    if (!current) continue;

    if (/^new file mode /.test(line)) {
      current.status = 'A';
      continue;
    }
    if (/^deleted file mode /.test(line)) {
      current.status = 'D';
      continue;
    }
    if (/^(old|new) mode /.test(line) || /^index /.test(line)) continue;

    const renameFrom = /^rename from (.+)$/.exec(line);
    if (renameFrom && renameFrom[1] && current) {
      current.status = 'R';
      current.previousPath = decodeGitPath(renameFrom[1]);
      continue;
    }
    const renameTo = /^rename to (.+)$/.exec(line);
    if (renameTo && renameTo[1] && current) {
      const to = decodeGitPath(renameTo[1]);
      current.path = to;
      current.absPath = to;
      continue;
    }

    const plusPlus = /^\+\+\+ (.+)$/.exec(line);
    if (plusPlus && plusPlus[1] && current && current.status !== 'R') {
      const p = decodeGitPath(plusPlus[1]).replace(/^b\//, '');
      if (p && p !== '/dev/null') {
        current.path = p;
        current.absPath = p;
      }
      continue;
    }

    const hunk = HUNK_RE.exec(line);
    if (hunk && current) {
      current.hunks.push({
        oldStart: parseInt(hunk[1] ?? '0', 10),
        oldCount: hunk[2] === undefined ? 1 : parseInt(hunk[2], 10),
        newStart: parseInt(hunk[3] ?? '0', 10),
        newCount: hunk[4] === undefined ? 1 : parseInt(hunk[4], 10),
      });
      continue;
    }
  }

  return files.map((f) => ({ ...f, hunks: f.hunks.filter((h) => h.newCount > 0) }));
}

/** A hunk covering an entire file's current content — used for untracked files. */
export function wholeFileHunk(text: string): DiffHunk {
  const count = Math.max(1, text.replace(/\n$/, '').split('\n').length);
  return { oldStart: 0, oldCount: 0, newStart: 1, newCount: count };
}

export function hunkLineRanges(file: DiffFile): Array<{ start: number; end: number }> {
  return file.hunks.map((h) => ({ start: h.newStart, end: h.newStart + h.newCount - 1 }));
}
