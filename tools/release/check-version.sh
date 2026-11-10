#!/usr/bin/env bash
# Fail unless a release tag is well formed and matches every version in the tree.
#
# Usage: tools/release/check-version.sh <tag> [repo-root]      (for example v2.0.0)
# Checks: the tag is vMAJOR.MINOR.PATCH with an optional -prerelease suffix, and equals the
# workspace version in Cargo.toml, app/src-tauri/tauri.conf.json and app/ui/package.json, the
# version of every workspace package in Cargo.lock, and the root version in
# app/ui/package-lock.json (top-level field and packages[""]).
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

# app/ui/package-lock.json (lockfile v3, two-space indent): the top-level "version" and the
# "version" of the root entry packages[""].
ui_lock_top="$(sed -n 's/^  "version":[[:space:]]*"\([^"]*\)".*/\1/p' "$root/app/ui/package-lock.json" | head -n1)"
ui_lock_root="$(awk '
  /^    "": \{/ { in_root = 1; next }
  in_root && /^    \}/ { exit }
  in_root && /^      "version":/ { gsub(/.*:[[:space:]]*"|".*/, ""); print; exit }
' "$root/app/ui/package-lock.json")"

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
check "app/ui/package-lock.json (top level)" "$ui_lock_top"
check "app/ui/package-lock.json (packages[\"\"])" "$ui_lock_root"

# Workspace packages are the members whose manifest says `version.workspace = true`; each must
# appear exactly once in Cargo.lock with the tag's version.
members=()
for manifest in "$root"/crates/*/Cargo.toml "$root/app/src-tauri/Cargo.toml"; do
  [ -f "$manifest" ] || continue
  grep -q '^version\.workspace[[:space:]]*=[[:space:]]*true' "$manifest" || continue
  members+=("$(sed -n 's/^name[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$manifest" | head -n1)")
done
if [ "${#members[@]}" -eq 0 ]; then
  echo "no workspace packages with version.workspace = true found" >&2
  status=1
fi
for name in "${members[@]}"; do
  lock_versions="$(awk -v want="name = \"$name\"" '
    $0 == want { getline; if ($1 == "version") { gsub(/"/, "", $3); print $3 } }
  ' "$root/Cargo.lock")"
  count="$(printf '%s' "$lock_versions" | grep -c . || true)"
  if [ "$count" -ne 1 ]; then
    echo "Cargo.lock: expected one entry for workspace package '$name', found $count" >&2
    status=1
    continue
  fi
  check "Cargo.lock ($name)" "$lock_versions"
done

if [ "$status" -eq 0 ]; then
  echo "tag $tag matches all versions (${#members[@]} workspace packages in Cargo.lock)"
fi
exit "$status"
