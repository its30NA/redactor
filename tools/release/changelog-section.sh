#!/usr/bin/env bash
# Print the CHANGELOG.md section of one version (the release notes body).
#
# Usage: tools/release/changelog-section.sh <version|tag> [changelog-file]
#   The version may carry a leading "v". The section starts at a heading such as
#   "## [2.0.0] - 2026-11-06" or "## 2.0.0" and ends before the next "## " heading
#   (or at the end of the file). The heading line itself is not printed.
#   Exits non-zero when the section is missing or has no content.
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "usage: $0 <version|tag> [changelog-file]" >&2
  exit 2
fi

version="${1#v}"
file="${2:-CHANGELOG.md}"

if [ ! -f "$file" ]; then
  echo "changelog not found: $file" >&2
  exit 1
fi

section="$(
  awk -v ver="$version" '
    # A version heading: "## [x.y.z]" or "## x.y.z", optionally followed by " - date" or text.
    /^##[[:space:]]/ {
      if (found) { exit }
      head = $0
      sub(/^##[[:space:]]+/, "", head)
      sub(/^\[/, "", head)
      split(head, parts, /[]][[:space:]]*|[[:space:]]+/)
      if (parts[1] == ver) { found = 1 }
      next
    }
    found { print }
  ' "$file"
)"

# Trim leading and trailing blank lines.
section="$(printf '%s\n' "$section" | awk 'NF { for (i = 0; i < blank; i++) print ""; blank = 0; started = 1; print; next } started { blank++ }')"

if [ -z "${section//[[:space:]]/}" ]; then
  echo "no CHANGELOG section with content for version $version in $file" >&2
  exit 1
fi

printf '%s\n' "$section"
