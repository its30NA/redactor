/** Builds the `scan` request and knows the 20 MiB input cap. */
import { mergeRanges, type Range } from './editor/positions';
import type { ScanRequest, Settings } from './types';

/** The engine's input cap, in UTF-8 bytes. */
export const MAX_INPUT_BYTES = 20 * 1024 * 1024;

/**
 * True when the text is over the cap whatever it contains: every UTF-16 code unit is at
 * least one UTF-8 byte, so the UI can refuse without counting bytes (and without sending
 * 20+ MiB over IPC). Texts under it may still be over in bytes; the shell then answers
 * with an `inputTooLarge` warning.
 */
export const definitelyTooLarge = (utf16Length: number): boolean => utf16Length > MAX_INPUT_BYTES;

/** UTF-8 byte length without allocating an encoded copy. */
export function utf8Length(text: string): number {
  let n = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    if (c < 0x80) n += 1;
    else if (c < 0x800) n += 2;
    else if (c >= 0xd800 && c <= 0xdbff && i + 1 < text.length) {
      n += 4;
      i++;
    } else n += 3;
  }
  return n;
}

export function buildScanRequest(
  text: string,
  settings: Pick<Settings, 'sensitivity' | 'pii' | 'stripInvisible'>,
  keep: Iterable<string>,
  manual: readonly Range[],
): ScanRequest {
  const clipped = manual.map((r) => ({ start: Math.max(0, r.start), end: Math.min(text.length, r.end) }));
  return {
    text,
    sensitivity: settings.sensitivity,
    pii: settings.pii,
    stripInvisible: settings.stripInvisible,
    keep: [...keep].sort(),
    manual: mergeRanges(clipped),
  };
}
