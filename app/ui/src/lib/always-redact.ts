/**
 * The "Always redact" list: plain words that are not passwords (names, company names,
 * internal hostnames). The list is stored on this PC in plain text, so an entry that looks
 * like a password gets a warning whose default action moves it to the session list, which is
 * never stored. Everything else that is wrong with an entry is an error.
 */
import { MAX_ALWAYS_REDACT, MIN_ALWAYS_REDACT } from './types';

/** Shortest single token that can look like a password. */
const MIN_PASSWORD_LENGTH = 8;

/** Label for a value the user typed into the Always redact field and kept for this session. */
export const TYPED_SECRET_LABEL = 'Typed secret';

/**
 * One token without whitespace, at least 8 characters, with three or more of: lower case,
 * upper case, digits, symbols (other than . - _). A company name with spaces never matches.
 */
export function looksLikePassword(raw: string): boolean {
  const word = raw.trim();
  if (word.length < MIN_PASSWORD_LENGTH || /\s/.test(word)) return false;
  const classes = [/[a-z]/, /[A-Z]/, /\d/, /[^\p{L}\p{N}.\-_\s]/u].filter((re) => re.test(word)).length;
  return classes >= 3;
}

export type WordCheck =
  | { kind: 'error'; message: string }
  | { kind: 'warn' }
  | { kind: 'ok'; list: string[] };

/** The outcome of adding `raw` to `list`: an error, a warning to confirm, or the new list. */
export function checkWord(list: readonly string[], raw: string): WordCheck {
  if (/[\r\n]/.test(raw)) return { kind: 'error', message: 'One line per entry.' };
  const word = raw.trim();
  if (word.length < MIN_ALWAYS_REDACT) return { kind: 'error', message: `At least ${MIN_ALWAYS_REDACT} characters.` };
  const lower = word.toLowerCase();
  if (list.some((w) => w.toLowerCase() === lower)) return { kind: 'error', message: 'Already on the list.' };
  if (list.length >= MAX_ALWAYS_REDACT)
    return { kind: 'error', message: `The list holds up to ${MAX_ALWAYS_REDACT} entries.` };
  if (looksLikePassword(word)) return { kind: 'warn' };
  return { kind: 'ok', list: [...list, word] };
}

/** `raw` appended to `list` without the password warning ("Add anyway"); other problems still refuse. */
export function addAnyway(list: readonly string[], raw: string): { list: string[] } | { error: string } {
  const res = checkWord(list, raw);
  if (res.kind === 'error') return { error: res.message };
  return { list: res.kind === 'ok' ? res.list : [...list, raw.trim()] };
}
