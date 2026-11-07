#!/usr/bin/env bash
# Fail unless a release tag is well formed and matches every version in the tree.
#
# Usage: tools/release/check-version.sh <tag> [repo-root]      (for example v2.0.0)
# Checks: the tag is vMAJOR.MINOR.PATCH with an optional -prerelease suffix, and equals the
# workspace version in Cargo.toml, app/src-tauri/tauri.conf.json and app/ui/package.json.
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "usage: $0 <tag> [repo-root]" >&2
  exit 2
fi

tag="$1"
root="${2:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"

if ! [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]]; then
  echo "tag '$tag' is not of the form vMAJOR.MINOR.PATCH[-prerelease]" >&2
  exit 1
fi
version="${tag#v}"

# First `version = "..."` line inside [workspace.package].
cargo_version="$(awk '
  /^\[workspace\.package\]/ { in_pkg = 1; next }
  /^\[/ { in_pkg = 0 }
  in_pkg && /^version[[:space:]]*=/ { gsub(/.*=[[:space:]]*"|".*/, ""); print; exit }
' "$root/Cargo.toml")"
tauri_version="$(sed -n 's/^[[:space:]]*"version":[[:space:]]*"\([^"]*\)".*/\1/p' "$root/app/src-tauri/tauri.conf.json" | head -n1)"
ui_version="$(sed -n 's/^[[:space:]]*"version":[[:space:]]*"\([^"]*\)".*/\1/p' "$root/app/ui/package.json" | head -n1)"

status=0
check() {
  if [ "$2" != "$version" ]; then
    echo "version mismatch: $1 is '$2', the tag says '$version'" >&2
    status=1
  fi
}
check "Cargo.toml [workspace.package]" "$cargo_version"
check "app/src-tauri/tauri.conf.json" "$tauri_version"
check "app/ui/package.json" "$ui_version"

if [ "$status" -eq 0 ]; then
  echo "tag $tag matches all versions"
fi
exit "$status"
