# Harsh test cases

Paste each block into the Redactor app, or pipe it through the CLI (`scrub case.txt`), and
check the result against the "Expected" note. These are deliberately confusing: glued-together
secrets, decoys, false-positive bait and overlapping matches.

Every value in this file is made up. Some are shaped like vendor tokens but are too short or
too regular to be real, and the engine checks lengths and checksums, so a pattern-only match
is not guaranteed for them. The notes below say what Redactor 2 does with each block as written.

---

### Test 1 - everything glued together, weird delimiters

```
key1:sk-abcdEFGH1234ijklMNOP5678,tok=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789;aws=AKIAIOSFODNN7EXAMPLE
```

**Expected:** the value after `key1:` and the GitHub token are redacted despite no spaces. The
AWS key ID is AWS's published documentation example, which Redactor leaves alone on purpose.

---

### Test 2 - false-positive bait (these must SURVIVE untouched)

```
commit 9f8e7d6c5b4a3f2e1d0c9b8a7654321000abcdef
request_id = 550e8400-e29b-41d4-a716-446655440000
version = 1.2.3.4 and build 2024.11.30
```

**Expected:** nothing redacted - a git SHA, a UUID, and dotted version numbers are not secrets.
With `--pii` the dotted version `1.2.3.4` is also a valid IPv4 address, so it is redacted as an
`IP Address`; that is the price of switching PII on.

---

### Test 3 - connection string with an email-shaped password (regression)

```
DATABASE_URL=postgres://admin:S3cr3t!Pass@db.example.test:5432/prod
```

**Expected:** the whole password is redacted as `Password in URL`; the fragment `S3cr3t` never
leaks. Run it with `--pii` (or the PII switch in the app) to see the resolver *merge* the
overlapping email-shaped match instead of exposing part of the value: the redaction then also
covers the host, `postgres://admin:[REDACTED: Password in URL]:5432/prod`.

---

### Test 4 - JWT hiding inside a Bearer header

```
curl -H "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ4In0.abcDEFghiJKLmno" https://api.example.test/v1
```

**Expected:** the token after `Bearer` is redacted as `JSON Web Token`. The sample is short but
well formed (three parts, and the header decodes to JSON with an `alg`), so the more specific JWT
detector wins over the generic Bearer one. A bearer value that is not a JWT is labelled
`Bearer Token`.

---

### Test 5 - secret with a custom name no tool knows natively

```
MEGACORP_INTERNAL_WIDGET_PASSWORD = h0rse-b4ttery-staple-99
```

**Expected:** redacted as a `Password`. No vendor pattern matches; the context engine reads
the whole key name (`...WIDGET_PASSWORD`) and judges the value.

---

### Test 6 - decoys that look secret but aren't

```
ENABLE_FEATURE=true
LOG_LEVEL=debug
API_TIMEOUT=30
RETRY_SECRET=changeme
AUTH_MODE=${VAULT_REF}
```

**Expected:** nothing redacted - `true`, `debug`, `30`, the placeholder `changeme`, and the
`${VAULT_REF}` template are all recognized as non-secrets.

---

### Test 7 - PEM private key inline inside JSON

```
{"tls_key": "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA1c3R1cGlkZXhhbXBsZWtleWRvbnR1c2U=\nsecond line of base64 body here\n-----END RSA PRIVATE KEY-----"}
```

**Expected:** the entire `BEGIN…END` block is redacted as one unit (`Private Key`).

---

### Test 8 - mixed provider soup on separate lines

```
export OPENAI_API_KEY=sk-abcdEFGH1234ijklMNOP5678qrstUVWX
export ANTHROPIC_API_KEY=sk-ant-api03-abcdEFGH1234ijklMNOP5678
GITHUB_TOKEN=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789
GOOGLE_API_KEY=AIzaBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB
SLACK_WEBHOOK=https://hooks.slack.com/services/T00000000/B00000000/XXXXXXXXXXXXXXXX
```

**Expected:** the first three lines are redacted: `OpenAI API Key #1` and `#2` for the two `sk-`
values, and a separate `GitHub Token`. The Anthropic sample is shorter than a real key (the rule
needs 32+ characters after `sk-ant-`), so it falls back to the generic `sk-` rule and is labelled as an
OpenAI key; a key of real length is labelled `Anthropic API Key`. The Google key and the Slack webhook here are made of
repeated characters, which Redactor treats as filler, so they stay. (A Stripe `sk_live_...` key
is intentionally left out of this file: secret scanners block it on sight.)

---

### Test 9 - turn PII on for this one (`--pii`, or the PII switch)

```
User jane.doe@example.test logged in from 10.0.0.5 with card 4111 1111 1111 1111 (SSN 123-45-6789).
An invalid card 1234 5678 9012 3456 should be ignored.
```

**Expected (PII on):** the email, the IP address, the Luhn-valid card and the SSN are redacted;
the invalid card `1234 5678 9012 3456` (it fails the Luhn checksum) is left untouched.

---

### Test 10 - duplicate values → stable numbering

```
primary_password=Tulip-Harbor-91
backup_password=Cedar-Quill-37
retry_password=Tulip-Harbor-91
```

**Expected:** `[REDACTED: Password #1]`, `#2`, then `#1` again. The same value reuses the same
number, so a reader can still see that `primary` and `retry` share a password.
