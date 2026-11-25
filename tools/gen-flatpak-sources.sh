#!/usr/bin/env bash
# Regenerate the Flatpak's offline sources from the lockfiles: every crate in Cargo.lock and every
# npm package in app/ui/package-lock.json as a download with its sha256, so flatpak-builder builds
# the app with no network.
#
# Usage: tools/gen-flatpak-sources.sh
# Writes packaging/flatpak/cargo-sources.json and packaging/flatpak/node-sources.json. Run it after
# any change to either lockfile; CI fails when the committed files differ from a fresh run. The
# output depends only on the lockfiles and the pinned generators, so a second run changes nothing.
#
# Needs python3 (with venv), git and the network (crates.io, the npm registry, PyPI, GitHub).
# The generators are flatpak-builder-tools at a pinned commit, kept in
# ${XDG_CACHE_HOME:-~/.cache}/redactor/flatpak-tools with a venv of their Python packages, which
# pip installs only with the hashes in tools/flatpak-requirements.txt.
set -euo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd)

# flatpak-builder-tools, master on 2026-11-25. Update by changing the commit and running this.
tools_url=https://github.com/flatpak/flatpak-builder-tools.git
tools_commit=9d86d336a39b9af38b86f114b6ce5820fe260f91

cache=${XDG_CACHE_HOME:-$HOME/.cache}/redactor/flatpak-tools
checkout=$cache/flatpak-builder-tools
venv=$cache/venv
out=$repo/packaging/flatpak
mkdir -p "$cache"

# The generators at the pinned commit, fetched alone (no history).
if [ "$(git -C "$checkout" rev-parse HEAD 2>/dev/null || true)" != "$tools_commit" ]; then
  echo "gen-flatpak-sources: flatpak-builder-tools at $tools_commit" >&2
  if [ ! -d "$checkout/.git" ]; then
    rm -rf "$checkout"
    git init -q "$checkout"
  fi
  git -C "$checkout" fetch -q --depth 1 "$tools_url" "$tools_commit"
  git -C "$checkout" -c advice.detachedHead=false checkout -q --force FETCH_HEAD
fi
[ "$(git -C "$checkout" rev-parse HEAD)" = "$tools_commit" ] || {
  echo "gen-flatpak-sources: $checkout is not at $tools_commit" >&2
  exit 1
}
git -C "$checkout" diff --quiet HEAD || {
  echo "gen-flatpak-sources: $checkout has local changes; delete the folder and run again" >&2
  exit 1
}

# Their Python packages, hash-checked; reinstalled when the requirements change.
requirements=$repo/tools/flatpak-requirements.txt
stamp=$(sha256sum "$requirements" | cut -d' ' -f1)
if [ "$(cat "$venv/.requirements-sha256" 2>/dev/null || true)" != "$stamp" ]; then
  echo "gen-flatpak-sources: Python packages for the generators" >&2
  rm -rf "$venv"
  python3 -m venv "$venv"
  "$venv/bin/pip" install --quiet --disable-pip-version-check --require-hashes --only-binary=:all: \
    -r "$requirements"
  printf '%s\n' "$stamp" >"$venv/.requirements-sha256"
fi

echo "gen-flatpak-sources: cargo sources from Cargo.lock" >&2
"$venv/bin/python" -I "$checkout/cargo/flatpak-cargo-generator.py" "$repo/Cargo.lock" \
  -o "$out/cargo-sources.json"

# The UI's dev dependencies are its build tools (Vite, Svelte), so they stay in. Playwright's
# browsers are for the UI tests only, never for the build.
echo "gen-flatpak-sources: npm sources from app/ui/package-lock.json" >&2
PYTHONPATH=$checkout/node "$venv/bin/python" -m flatpak_node_generator npm \
  --no-playwright-browsers \
  "$repo/app/ui/package-lock.json" -o "$out/node-sources.json"

ls -l "$out/cargo-sources.json" "$out/node-sources.json" >&2
