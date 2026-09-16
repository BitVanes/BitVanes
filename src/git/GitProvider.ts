import { spawn } from 'child_process';
import * as path from 'path';
import * as vscode from 'vscode';
import type { Range } from '../types/protocol';
import { indentOf, expandToBlock } from '../util/text';
import { parseUnifiedDiff } from './pure';
import type { DiffFile } from './pure';

export { parseUnifiedDiff, hunkLineRanges } from './pure';
export type { DiffFile, DiffHunk } from './pure';

export interface SelectionTarget {
  filePath: string;
  absPath: string;
  range: Range;
  text: string;
  documentText: string;
}

export class GitProvider {
  static async exec(args: string[], cwd: string, timeoutMs = 15_000): Promise<string> {
    return new Promise((resolve, reject) => {
      const child = spawn('git', args, {
        cwd,
        windowsHide: true,
        env: { ...process.env, GIT_PAGER: 'cat', PAGER: 'cat' },
      });
      let stdout = '';
      let stderr = '';
      const timer = setTimeout(() => {
        child.kill();
        reject(new Error(`git ${args[0]} timed out after ${timeoutMs}ms`));
      }, timeoutMs);
      child.stdout.on('data', (d: Buffer) => {
        stdout += d.toString('utf8');
      });
      child.stderr.on('data', (d: Buffer) => {
        stderr += d.toString('utf8');
      });
      child.on('error', (err) => {
        clearTimeout(timer);
        reject(err);
      });
      child.on('close', (code) => {
        clearTimeout(timer);
        if (code === 0) resolve(stdout);
        else reject(new Error(`git ${args.join(' ')} exited with ${code}: ${stderr.trim().slice(0, 400)}`));
      });
    });
  }

  static workspaceRoot(): string | undefined {
    const folder = vscode.workspace.workspaceFolders?.find((f) => f.uri.scheme === 'file');
    return folder?.uri.fsPath;
  }

  static async isGitRepo(root: string): Promise<boolean> {
    try {
      const out = await GitProvider.exec(['rev-parse', '--is-inside-work-tree'], root);
      return out.trim() === 'true';
    } catch {
      return false;
    }
  }

  static async diff(root: string, staged: boolean): Promise<DiffFile[]> {
    const args = ['diff', '--unified=0', '--no-color', '--no-ext-diff'];
    if (staged) args.push('--cached');
    const out = await GitProvider.exec(args, root);
    const files = parseUnifiedDiff(out);
    return files.filter((f) => f.hunks.length > 0).map((f) => ({ ...f, absPath: path.resolve(root, f.path) }));
  }
}

export function getActiveSelection(): SelectionTarget | undefined {
  const editor = vscode.window.activeTextEditor;
  if (!editor) return undefined;
  const doc = editor.document;
  const sel = editor.selection;

  let range: Range;
  let body: string;
  if (!sel.isEmpty) {
    range = {
      startLine: sel.start.line + 1,
      startCol: sel.start.character + 1,
      endLine: sel.end.line + 1,
      endCol: sel.end.character + 1,
    };
    body = doc.getText(sel);
  } else {
    const lines = doc.getText().split('\n');
    const block = expandToBlock(lines, sel.active.line);
    range = {
      startLine: block.startLine + 1,
      startCol: Math.max(1, indentOf(lines[block.startLine] ?? '') + 1),
      endLine: block.endLine + 1,
      endCol: Math.max(1, (lines[block.endLine] ?? '').trimEnd().length + 1),
    };
    body = lines.slice(block.startLine, block.endLine + 1).join('\n');
  }

  const relPath = vscode.workspace.asRelativePath(doc.uri, false);
  return {
    filePath: relPath.replace(/\\/g, '/'),
    absPath: doc.uri.fsPath,
    range,
    text: body,
    documentText: doc.getText(),
  };
}
