import { describe, expect, it } from 'vitest';
import { parseUnifiedDiff } from '../src/git/pure';

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
