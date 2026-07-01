# Harsh test cases

Paste each block into `scrub ui` (or `pbpaste | scrub`) and check the result against the
"Expected" note. These are deliberately confusing — glued-together secrets, decoys,
false-positive bait, and overlapping matches.

---

### Test 1 — everything glued together, weird delimiters

```
key1:sk-abcdEFGH1234ijklMNOP5678,tok=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789;aws=AKIAIOSFODNN7EXAMPLE
```

**Expected:** all three redacted (OpenAI key, GitHub token, AWS Access Key ID) despite no spaces.

---

### Test 2 — false-positive bait (these must SURVIVE untouched)

```
commit 9f8e7d6c5b4a3f2e1d0c9b8a7654321000abcdef
request_id = 550e8400-e29b-41d4-a716-446655440000
version = 1.2.3.4 and build 2024.11.30
```

**Expected:** nothing redacted — a git SHA, a UUID, and dotted version numbers are not secrets.

---

### Test 3 — connection string with an email-shaped password (regression)

```
DATABASE_URL=postgres://admin:S3cr3t!Pass@db.internal.corp:5432/prod
```

**Expected:** the whole password is redacted; the fragment `S3cr3t` never leaks. Toggle PII
on to see the resolver *merge* the overlapping email match instead of exposing part of the value.

---

### Test 4 — JWT hiding inside a Bearer header

```
curl -H "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ4In0.abcDEFghiJKLmno" https://api.internal.corp/v1
```

**Expected:** labeled `JSON Web Token (JWT)` — the more specific detector wins over generic Bearer.

---

### Test 5 — secret with a custom name no tool knows natively

```
MEGACORP_INTERNAL_WIDGET_PASSWORD = h0rse-b4ttery-staple-99
```

**Expected:** redacted as a `Password` (caught by the suspicious variable name).

---

### Test 6 — decoys that look secret but aren't

```
ENABLE_FEATURE=true
LOG_LEVEL=debug
API_TIMEOUT=30
RETRY_SECRET=changeme
AUTH_MODE=${VAULT_REF}
```

**Expected:** nothing redacted — `true`, `debug`, `30`, the placeholder `changeme`, and the
`${VAULT_REF}` template are all recognized as non-secrets.

---

### Test 7 — PEM private key inline inside JSON

```
{"tls_key": "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA1c3R1cGlkZXhhbXBsZWtleWRvbnR1c2U=\nsecond line of base64 body here\n-----END RSA PRIVATE KEY-----"}
```

**Expected:** the entire `BEGIN…END` block is redacted as one unit (`Private Key`).

---

### Test 8 — mixed provider soup on separate lines

```
export OPENAI_API_KEY=sk-abcdEFGH1234ijklMNOP5678qrstUVWX
export ANTHROPIC_API_KEY=sk-ant-api03-abcdEFGH1234ijklMNOP5678
GITHUB_TOKEN=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789
GOOGLE_API_KEY=AIzaBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB
SLACK_WEBHOOK=https://hooks.slack.com/services/T00000000/B00000000/XXXXXXXXXXXXXXXX
```

**Expected:** five distinct redactions with correct, distinct labels; note Anthropic is *not*
mislabeled as OpenAI despite the shared `sk-` prefix. (A Stripe `sk_live_…` key is
intentionally left out of this file — GitHub's own push-protection blocks it, which is a
nice real-world demonstration of exactly what redactor prevents.)

---

### Test 9 — turn ON the PII checkbox for this one

```
User jane.doe@corp.io logged in from 10.0.0.5 with card 4111 1111 1111 1111 (SSN 123-45-6789).
An invalid card 1234 5678 9012 3456 should be ignored.
```

**Expected (PII on):** email, IP, the Luhn-valid card, and the SSN are redacted; the invalid
card `1234 5678 9012 3456` (fails the Luhn checksum) is left untouched.

---

### Test 10 — duplicate values → stable numbering

```
primary=sk-aaaaAAAA1111bbbbBBBB2222 backup=sk-ccccCCCC3333ddddDDDD4444 retry=sk-aaaaAAAA1111bbbbBBBB2222
```

**Expected:** `[REDACTED: OpenAI API Key #1]`, `#2`, then `#1` again — the same key reuses the
same number so an assistant can still see "primary and retry use the same key".
