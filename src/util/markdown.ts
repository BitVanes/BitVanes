/** Escape untrusted text (model output, file paths) before embedding in MarkdownString. */
export function mdEscape(s: string): string {
  return s.replace(/([\\`*_{}[\]()#+!~|<>])/g, '\\$1');
}

/** Make a string safe inside a single-line inline code span. */
export function mdInline(s: string): string {
  return s.replace(/`/g, "'").replace(/\n/g, ' ').slice(0, 120);
}
