/**
 * PDF text extraction via Mozilla PDF.js.
 *
 * Runs entirely in the browser. PDF.js decodes the PDF binary and extracts
 * text with positional/font information. We convert this to Markdown with
 * heading detection based on font sizes, then feed it to the Rust engine
 * as `format: 'markdown'`.
 *
 * No data leaves the browser — PDF.js runs client-side.
 */

import * as pdfjsLib from 'pdfjs-dist';
// Vite handles the ?url suffix — returns the bundled worker URL.
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';

pdfjsLib.GlobalWorkerOptions.workerSrc = workerUrl;

interface TextItemData {
  str: string;
  x: number;
  y: number;
  height: number;
  fontName: string;
}

/**
 * Extracts text from a PDF and converts it to Markdown.
 *
 * Headings are detected by font size: items significantly larger than
 * the median become H1/H2 headings. This gives the engine's heading-path
 * ancestry feature when processing PDFs.
 *
 * @returns Markdown-formatted string (UTF-8 compatible).
 */
export async function extractPdfText(bytes: Uint8Array): Promise<string> {
  const loadingTask = pdfjsLib.getDocument({
    data: bytes,
    // Disable annotation rendering (we only want text).
    disableAutoFetch: false,
    disableStream: false,
  });

  const doc = await loadingTask.promise;
  const allItems: TextItemData[] = [];

  for (let pageNum = 1; pageNum <= doc.numPages; pageNum++) {
    const page = await doc.getPage(pageNum);
    const content = await page.getTextContent();

    for (const item of content.items) {
      if (!('str' in item) || !('transform' in item)) continue;
      const ti = item as { str: string; transform: number[]; height: number; fontName: string };
      if (ti.str.trim().length === 0) continue;

      allItems.push({
        str: ti.str,
        x: ti.transform[4],
        y: ti.transform[5],
        height: ti.height,
        fontName: ti.fontName,
      });
    }
    await page.cleanup();
  }

  await loadingTask.destroy();

  if (allItems.length === 0) {
    return '';
  }

  return itemsToMarkdown(allItems);
}

/**
 * Converts positioned text items into Markdown with heading detection.
 *
 * Algorithm:
 * 1. Group items into lines by Y position.
 * 2. Calculate the median font height (body text size).
 * 3. Lines with font height > 1.4× median → H1.
 * 4. Lines with font height > 1.15× median → H2.
 * 5. Group lines into paragraphs (separated by Y gaps).
 */
function itemsToMarkdown(items: TextItemData[]): string {
  if (items.length === 0) return '';

  // 1. Group into lines by similar Y position.
  const lines: { text: string; y: number; avgHeight: number }[] = [];
  let currentLine: { strs: string[]; y: number; heights: number[] } | null = null;

  // Sort by Y descending (PDFs are top-to-bottom, Y increases upward).
  const sorted = [...items].sort((a, b) => b.y - a.y || a.x - b.x);

  for (const item of sorted) {
    if (currentLine && Math.abs(currentLine.y - item.y) < 3) {
      currentLine.strs.push(item.str);
      currentLine.heights.push(item.height);
    } else {
      if (currentLine) {
        lines.push({
          text: currentLine.strs.join(''),
          y: currentLine.y,
          avgHeight: currentLine.heights.reduce((a, b) => a + b, 0) / currentLine.heights.length,
        });
      }
      currentLine = { strs: [item.str], y: item.y, heights: [item.height] };
    }
  }
  if (currentLine) {
    lines.push({
      text: currentLine.strs.join(''),
      y: currentLine.y,
      avgHeight: currentLine.heights.reduce((a, b) => a + b, 0) / currentLine.heights.length,
    });
  }

  // 2. Calculate median font height.
  const heights = lines.map((l) => l.avgHeight).sort((a, b) => a - b);
  const median = heights[Math.floor(heights.length / 2)] || 10;

  // 3. Convert lines to markdown.
  const parts: string[] = [];
  let prevY: number | null = null;

  for (const line of lines) {
    const text = line.text.trim();
    if (!text) continue;

    // Detect paragraph breaks (large Y gap between lines).
    if (prevY !== null) {
      const gap = prevY - line.y;
      if (gap > median * 0.8) {
        parts.push(''); // blank line = paragraph separator
      }
    }

    // Heading detection by font size.
    const ratio = line.avgHeight / median;
    if (ratio > 1.4) {
      parts.push(`# ${text}`);
    } else if (ratio > 1.15) {
      parts.push(`## ${text}`);
    } else {
      parts.push(text);
    }

    prevY = line.y;
  }

  return parts.join('\n');
}
