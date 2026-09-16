import { describe, expect, it } from 'vitest';
import { decodeGitPath, parseUnifiedDiff, wholeFileHunk } from '../src/git/pure';

const DIFF = `diff --git a/src/lib.rs b/src/lib.rs
index 1234567..89abcde 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -12 +12 @@
-let old = 1;
+let new = 2;
@@ -40,3 +40,5 @@ impl Vault {
     let a = 1;
-    let b = 2;
+    let b = 3;
+    let c = 4;
+    let d = 5;
diff --git a/contracts/Vault.sol b/contracts/Vault.sol
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ b/contracts/Vault.sol
@@ -0,0 +1,3 @@
+contract Vault {
+    uint256 public total;
+}
diff --git a/old_name.py b/renamed.py
similarity index 90%
rename from old_name.py
rename to renamed.py
index aaa..bbb 100644
--- a/old_name.py
+++ b/renamed.py
@@ -5,2 +5,2 @@
-x = 1
+x = 2
 y = 3
diff --git a/gone.go b/gone.go
deleted file mode 100644
index ccc..0000000
--- a/gone.go
+++ /dev/null
@@ -1,2 +0,0 @@
-package main
-func main() {}
`;

describe('parseUnifiedDiff', () => {
  const files = parseUnifiedDiff(DIFF);

  it('parses all files', () => {
    expect(files.map((f) => f.path)).toEqual([
      'src/lib.rs',
      'contracts/Vault.sol',
      'renamed.py',
      'gone.go',
    ]);
  });

  it('handles single-line hunks without a count', () => {
    const lib = files[0]!;
    expect(lib.hunks[0]).toEqual({ oldStart: 12, oldCount: 1, newStart: 12, newCount: 1 });
    expect(lib.hunks[1]).toEqual({ oldStart: 40, oldCount: 3, newStart: 40, newCount: 5 });
    expect(lib.status).toBe('M');
  });

  it('marks new files as added', () => {
    const sol = files[1]!;
    expect(sol.status).toBe('A');
    expect(sol.hunks[0]?.newCount).toBe(3);
  });

  it('tracks renames with previousPath', () => {
    const py = files[2]!;
    expect(py.status).toBe('R');
    expect(py.previousPath).toBe('old_name.py');
    expect(py.path).toBe('renamed.py');
  });

  it('drops deleted files that have no new-side lines', () => {
    const go = files[3]!;
    expect(go.status).toBe('D');
    expect(go.hunks.length).toBe(0);
  });

  it('returns empty for non-diff input', () => {
    expect(parseUnifiedDiff('hello world')).toEqual([]);
  });
});

// The following fixtures match real `git diff` output shapes (verified against git):
// - space-containing paths get a trailing TAB on the +++ line
// - non-ASCII paths are C-quoted with octal escapes on both diff --git and +++
describe('parseUnifiedDiff path quoting', () => {
  it('parses paths containing spaces (trailing TAB marker)', () => {
    const diff = [
      'diff --git a/sp ace.rs b/sp ace.rs',
      'index 1234567..89abcde 100644',
      '--- a/sp ace.rs',
      '+++ b/sp ace.rs\t',
      '@@ -1 +1 @@',
      '-old',
      '+new',
    ].join('\n');
    const files = parseUnifiedDiff(diff);
    expect(files.length).toBe(1);
    expect(files[0]!.path).toBe('sp ace.rs');
  });

  it('parses non-ASCII paths (quoted with octal escapes)', () => {
    const diff = [
      String.raw`diff --git "a/\303\251t\303\251.rs" "b/\303\251t\303\251.rs"`,
      'new file mode 100644',
      'index 0000000..89abcde',
      '--- /dev/null',
      String.raw`+++ "b/\303\251t\303\251.rs"`,
      '@@ -0,0 +1 @@',
      '+fn b(){}',
    ].join('\n');
    const files = parseUnifiedDiff(diff);
    expect(files.length).toBe(1);
    expect(files[0]!.path).toBe('été.rs');
    expect(files[0]!.status).toBe('A');
  });

  it('decodes quoted rename paths', () => {
    const diff = [
      String.raw`diff --git "a/old \303\251t\303\251.rs" "b/new \303\251t\303\251.rs"`,
      'similarity index 90%',
      String.raw`rename from "old \303\251t\303\251.rs"`,
      String.raw`rename to "new \303\251t\303\251.rs"`,
      'index aaa..bbb 100644',
      '--- a/old file.rs',
      '+++ b/new file.rs\t',
      '@@ -5 +5 @@',
      '-x = 1',
      '+x = 2',
    ].join('\n');
    const files = parseUnifiedDiff(diff);
    expect(files[0]!.status).toBe('R');
    expect(files[0]!.previousPath).toBe('old été.rs');
    expect(files[0]!.path).toBe('new été.rs');
  });
});

describe('decodeGitPath', () => {
  it('unquotes and unescapes octal', () => {
    expect(decodeGitPath('"b/\\303\\251t\\303\\251.rs"')).toBe('b/été.rs');
  });

  it('strips the trailing TAB marker', () => {
    expect(decodeGitPath('b/sp ace.rs\t')).toBe('b/sp ace.rs');
  });

  it('leaves plain paths untouched', () => {
    expect(decodeGitPath('src/lib.rs')).toBe('src/lib.rs');
  });
});

describe('wholeFileHunk', () => {
  it('covers every line of the file', () => {
    expect(wholeFileHunk('a\nb\nc\n')).toEqual({ oldStart: 0, oldCount: 0, newStart: 1, newCount: 3 });
  });

  it('handles single-line files without a trailing newline', () => {
    expect(wholeFileHunk('a')).toEqual({ oldStart: 0, oldCount: 0, newStart: 1, newCount: 1 });
  });

  it('never produces an empty hunk', () => {
    expect(wholeFileHunk('').newCount).toBe(1);
  });
});
