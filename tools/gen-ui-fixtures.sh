#!/usr/bin/env bash
# Regenerates the UI's mock fixtures from the real engine.
#
#   scan-{demo,harsh,empty}.json   `scrub --json` over the demo inputs of app/ui/src/lib/mock/demo.ts
#   app-info.json                  ruleCount from `scrub version` (the engine's own rule count)
#
# The demo texts come from app/ui/scripts/demo-fixture.ts, which assembles the fake tokens at
# runtime, so no token is ever a contiguous literal in the repo. Per-run values are normalised so
# that regenerating gives byte-identical files: valueKey (random per process) becomes a stable
# key derived from the order of distinct values, and stats.ms becomes 3.4.
#
# Usage: tools/gen-ui-fixtures.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UI="$ROOT/app/ui"
OUT="$UI/src/lib/mock/fixtures"

cd "$ROOT"
cargo build -q -p scrub
SCRUB="$ROOT/target/debug/scrub"

# Normalises one scan response read from stdin and prints it the way the fixtures are stored.
normalize() {
  node --input-type=module -e '
    import { createHash } from "node:crypto";
    import { readFileSync } from "node:fs";
    const name = process.argv[1];
    const res = JSON.parse(readFileSync(0, "utf8"));
    const keys = new Map();
    for (const f of res.findings) {
      if (!keys.has(f.valueKey)) {
        const n = keys.size;
        keys.set(f.valueKey, createHash("sha256").update(`fixture:${name}:${n}`).digest("hex").slice(0, 32));
      }
      f.valueKey = keys.get(f.valueKey);
    }
    res.stats.ms = res.findings.length ? 3.4 : 0;
    process.stdout.write(JSON.stringify(res, null, 2) + "\n");
  ' "$1"
}

for name in demo harsh empty; do
  node "$UI/scripts/demo-fixture.ts" "$name" \
    | "$SCRUB" --json -c /dev/null \
    | normalize "$name" > "$OUT/scan-$name.json.tmp"
  mv "$OUT/scan-$name.json.tmp" "$OUT/scan-$name.json"
  echo "wrote scan-$name.json"
done

# ruleCount: the engine's rule count, the number the app reports in appInfo.
rules="$("$SCRUB" version | sed -n 's/.*, \([0-9][0-9]*\) rules).*/\1/p')"
[ -n "$rules" ] || { echo "could not read the rule count from 'scrub version'" >&2; exit 1; }
node --input-type=module -e '
  import { readFileSync, writeFileSync } from "node:fs";
  const [file, rules] = process.argv.slice(1);
  const info = JSON.parse(readFileSync(file, "utf8"));
  info.ruleCount = Number(rules);
  writeFileSync(file, JSON.stringify(info, null, 2) + "\n");
' "$OUT/app-info.json" "$rules"
echo "app-info.json: ruleCount $rules"
