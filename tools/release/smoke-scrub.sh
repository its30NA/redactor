#!/usr/bin/env bash
# Smoke-test a built scrub binary: it prints its version, and a made-up secret piped through
# it comes out replaced by a [REDACTED placeholder.
#
# Usage: tools/release/smoke-scrub.sh <path-to-scrub> [runner-prefix...]
# A runner prefix runs the binary through another program, for example `arch -x86_64`.
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: $0 <path-to-scrub> [runner-prefix...]" >&2
  exit 2
fi

bin="$1"
shift
# Made up for this test; it is no real credential.
secret='Plum-tree47-example'

"$@" "$bin" version
out="$(printf 'password = %s\n' "$secret" | "$@" "$bin")"

if grep -qF -- "$secret" <<<"$out"; then
  echo "smoke test failed: the secret is still in the output" >&2
  exit 1
fi
if ! grep -qF -- '[REDACTED' <<<"$out"; then
  echo "smoke test failed: no [REDACTED placeholder in the output: $out" >&2
  exit 1
fi
echo "smoke test passed: $out"
