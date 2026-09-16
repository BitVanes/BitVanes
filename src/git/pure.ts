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

export function parseUnifiedDiff(text: string): DiffFile[] {
  const files: DiffFile[] = [];
  let current: DiffFile | null = null;

  for (const line of text.split('\n')) {
    const diffGit = /^diff --git (?:"?a\/(.+?)"?) (?:"?b\/(.+?)"?)$/.exec(line);
    if (diffGit && diffGit[1] && diffGit[2]) {
      current = {
        path: diffGit[2],
        absPath: diffGit[2],
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
      current.previousPath = renameFrom[1];
      continue;
    }
    const renameTo = /^rename to (.+)$/.exec(line);
    if (renameTo && renameTo[1] && current) {
      current.path = renameTo[1];
      current.absPath = renameTo[1];
      continue;
    }

    const plusPlus = /^\+\+\+ (?:b\/)?(.+)$/.exec(line);
    if (plusPlus && plusPlus[1] && plusPlus[1] !== '/dev/null' && current && current.status !== 'R') {
      current.path = plusPlus[1];
      current.absPath = plusPlus[1];
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

export function hunkLineRanges(file: DiffFile): Array<{ start: number; end: number }> {
  return file.hunks.map((h) => ({ start: h.newStart, end: h.newStart + h.newCount - 1 }));
}
