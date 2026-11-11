#!/usr/bin/env bash
# Package a built scrub binary (Linux and macOS) as a .tar.gz with the license, third-party notices and README.
#
# Usage: tools/release/package-scrub.sh <tag> <target-triple> <out-dir>
# Expects target/<target-triple>/release/scrub. Writes <out-dir>/scrub-<tag>-<target-triple>.tar.gz.
set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <tag> <target-triple> <out-dir>" >&2
  exit 2
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tag="$1"
target="$2"
out="$3"
bin="${SCRUB_TARGET_DIR:-$root/target}/$target/release/scrub"
name="scrub-$tag-$target"

if [ ! -f "$bin" ]; then
  echo "missing binary: $bin" >&2
  exit 1
fi

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/$name" "$out"
cp "$bin" "$stage/$name/scrub"
cp "$root/LICENSE" "$root/THIRD_PARTY_NOTICES.md" "$root/README.md" "$stage/$name/"
chmod 755 "$stage/$name/scrub"
tar -C "$stage" -czf "$out/$name.tar.gz" "$name"
echo "$out/$name.tar.gz"
