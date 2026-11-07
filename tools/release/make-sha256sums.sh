#!/usr/bin/env bash
# Write SHA256SUMS for every file in a directory (sha256sum format, names relative to it).
#
# Usage: tools/release/make-sha256sums.sh <dir>
# Writes <dir>/SHA256SUMS, sorted by name, without hashing itself, and verifies it right away.
# Fails when the directory has no files or contains subdirectories.
set -euo pipefail

if [ "$#" -ne 1 ] || [ ! -d "$1" ]; then
  echo "usage: $0 <dir>" >&2
  exit 2
fi

cd "$1"
rm -f SHA256SUMS

if find . -mindepth 1 -type d | grep -q .; then
  echo "refusing: $1 contains subdirectories; artifacts must sit directly in it" >&2
  exit 1
fi

mapfile -t files < <(find . -maxdepth 1 -type f -printf '%f\n' | LC_ALL=C sort)
if [ "${#files[@]}" -eq 0 ]; then
  echo "no files to hash in $1" >&2
  exit 1
fi

sha256sum -- "${files[@]}" > SHA256SUMS
sha256sum --check --quiet SHA256SUMS
echo "SHA256SUMS: ${#files[@]} files"
