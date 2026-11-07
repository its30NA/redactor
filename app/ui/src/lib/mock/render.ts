/**
 * Splices placeholders into a text, the way the engine's redact step does,
 * for the mock backend. Only `import type` here, so plain Node
 * can run it. The real engine is the source of truth; this exists so dev mode can honour
 * `keep` and `manual` without a backend.
 */
import type { Finding, OutputSpan } from '../types';

export interface Rendered {
  output: string;
  outputSpans: OutputSpan[];
}

/** `findings` must be sorted by start and non-overlapping (the engine's resolve step guarantees it). */
export function render(text: string, findings: readonly Finding[]): Rendered {
  let output = '';
  let cursor = 0;
  const outputSpans: OutputSpan[] = [];
  for (const f of findings) {
    output += text.slice(cursor, f.start);
    if (f.kept) {
      output += text.slice(f.start, f.end);
    } else {
      const start = output.length;
      output += f.placeholder;
      outputSpans.push({ start, end: output.length, findingId: f.id });
    }
    cursor = f.end;
  }
  output += text.slice(cursor);
  return { output, outputSpans };
}

/** 1-based line number of a UTF-16 offset. */
export function lineAt(text: string, offset: number): number {
  let line = 1;
  for (let i = 0; i < offset && i < text.length; i++) if (text.charCodeAt(i) === 10) line++;
  return line;
}
