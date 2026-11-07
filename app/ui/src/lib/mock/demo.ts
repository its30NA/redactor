/**
 * Demo input texts for the mock backend.
 *
 * The repo is public and scanned by secret scanners, so no file may contain a contiguous,
 * realistic vendor token. Every fake secret here is assembled at runtime from parts, with
 * bodies drawn from a seeded PRNG: deterministic (the fixtures' offsets stay valid) but never
 * written out literally. This module has no imports so `scripts/demo-fixture.ts` can run it
 * under plain Node.
 */

const UPPER = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ';
const LOWER = 'abcdefghijklmnopqrstuvwxyz';
const DIGITS = '0123456789';
const B62 = UPPER + LOWER + DIGITS;
const B64 = B62 + '+/';
const B64URL = B62 + '-_';

/** mulberry32: tiny, seeded, good enough for fake token bodies. */
function prng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function body(seed: number, length: number, alphabet: string): string {
  const next = prng(seed);
  let out = '';
  for (let i = 0; i < length; i++) out += alphabet[Math.floor(next() * alphabet.length)];
  return out;
}

/** base64url without padding (the JWT parts). */
const b64url = (json: string) => btoa(json).replace(/=+$/, '').replace(/\+/g, '-').replace(/\//g, '_');

/** The fake secrets inside the demo text, keyed by role (the unit tests look values up by role). */
export function demoSecrets() {
  return {
    dbPassword: ['Orchid', 'Lantern', '42!'].join('-'),
    openaiKey: ['sk', 'proj', body(11, 48, B62)].join('-'),
    githubToken: ['gh', 'p'].join('') + '_' + body(12, 36, B62),
    awsKeyId: ['AK', 'IA'].join('') + body(13, 16, UPPER + '234567'),
    awsSecret: body(14, 40, B64),
    curlPassword: ['b4ckup', 'Night', '7'].join('-'),
    wifiPassword: ['Sommer', 'Regen', '2026', '#'].join(''),
    signingSecret: body(16, 32, B62),
    ticketBlob: body(15, 28, B62),
  };
}

/**
 * A realistic mixed paste: an .env block, a shell line, a log line that repeats a password,
 * German prose, a chat line with an emoji glued to a secret, an unlabeled random string, a
 * CRLF block saved on Windows (it reuses the curl password), and false-positive bait.
 * The emoji put astral characters before the secrets and the CRLF lines put CRs before the
 * last ones, so every highlight position depends on exact UTF-16 offsets.
 */
export function demoInput(): string {
  const s = demoSecrets();
  return [
    '# 🚀 staging rollout · notes for the on-call channel',
    `DATABASE_URL=postgres://orders_app:${s.dbPassword}@db.staging.example:5432/orders`,
    `OPENAI_API_KEY=${s.openaiKey}`,
    `export GITHUB_TOKEN=${s.githubToken}`,
    `aws_access_key_id = ${s.awsKeyId}`,
    `aws_secret_access_key = ${s.awsSecret}`,
    '',
    `$ curl -u deploy:${s.curlPassword} https://ci.example.test/api/v1/jobs`,
    `2026-11-06T09:14:02Z WARN auth retry for orders_app with password ${s.dbPassword}`,
    `Das Passwort fürs Staging-WLAN ist ${s.wifiPassword} – bitte nicht weitergeben.`,
    `@oncall webhook signing secret, rotate after the demo 🔑${s.signingSecret}`,
    `ticket 4471 attachment note: ${s.ticketBlob}`,
    '',
    '[smtp]\r',
    'host = mail.example.test\r',
    `password = ${s.curlPassword}\r`,
    '\r',
    'request_id = 550e8400-e29b-41d4-a716-446655440000',
    'commit 9f8e7d6c5b4a3f2e1d0c9b8a7654321000abcdef',
    '',
  ].join('\n');
}

/** The fake secrets of the HARSH_TESTS port, keyed by role. */
export function harshSecrets() {
  return {
    legacyOpenai: ['sk', body(21, 32, B62)].join('-'),
    githubToken: ['gh', 'p'].join('') + '_' + body(22, 36, B62),
    awsKeyId: ['AK', 'IA'].join('') + body(23, 16, UPPER + '234567'),
    dbPassword: ['S3cr3t', 'Pass'].join('!'),
    jwt: [b64url('{"alg":"HS256"}'), b64url('{"sub":"demo"}'), body(24, 22, B64URL)].join('.'),
    widgetPassword: ['h0rse', 'b4ttery', 'staple', '99'].join('-'),
  };
}

/**
 * `docs/HARSH_TESTS.md` cases 1–6 with every token assembled at runtime: glued secrets,
 * false-positive bait that must survive, an email-shaped password, a JWT behind Bearer, a
 * secret name no tool knows, and decoys.
 */
export function harshInput(): string {
  const s = harshSecrets();
  return [
    '# 1 · glued together, odd delimiters',
    `key1:${s.legacyOpenai},tok=${s.githubToken};aws=${s.awsKeyId}`,
    '',
    '# 2 · false-positive bait (must survive)',
    'commit 9f8e7d6c5b4a3f2e1d0c9b8a7654321000abcdef',
    'request_id = 550e8400-e29b-41d4-a716-446655440000',
    'version = 1.2.3.4 and build 2024.11.30',
    '',
    '# 3 · connection string, email-shaped password',
    `DATABASE_URL=postgres://admin:${s.dbPassword}@db.internal.example:5432/prod`,
    '',
    '# 4 · JWT behind a Bearer header',
    `curl -H "Authorization: Bearer ${s.jwt}" https://api.internal.example/v1`,
    '',
    '# 5 · a secret name no tool knows',
    `MEGACORP_INTERNAL_WIDGET_PASSWORD = ${s.widgetPassword}`,
    '',
    '# 6 · decoys (must survive)',
    'ENABLE_FEATURE=true',
    'LOG_LEVEL=debug',
    'API_TIMEOUT=30',
    'RETRY_SECRET=changeme',
    'AUTH_MODE=${VAULT_REF}',
    '',
  ].join('\n');
}

/** Named demo inputs; the mock serves `fixtures/scan-<name>.json` when the scanned text equals one. */
export const DEMO_INPUTS: Record<string, () => string> = {
  demo: demoInput,
  harsh: harshInput,
  empty: () => '',
};

/** The demo text repeated to at least `units` UTF-16 code units. */
export function tiledDemo(units: number): string {
  const d = demoInput();
  return d.repeat(Math.ceil(units / d.length));
}

/**
 * Big pastes for the responsiveness check (5 MiB) and the 20 MiB cap notice. They have no
 * fixture: the mock tiles the demo response over them.
 */
export const BIG_INPUTS: Record<string, () => string> = {
  large: () => tiledDemo(5 * 1024 * 1024),
  huge: () => tiledDemo(20 * 1024 * 1024 + 1),
};
