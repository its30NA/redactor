#!/usr/bin/env bash
# Build the Flatpak from source and export it as one file, the way a release does. CI runs it on
# every pull request and release.yml for the release.
#
# Usage: tools/release/bundle-flatpak.sh <out-dir>
# Runs where flatpak-builder and the GNOME 51 SDK are (CI: the flathub-infra gnome-51 container,
# as root, with --privileged for bubblewrap). Needs the network for the sources and, if missing,
# the runtime; the build itself runs offline (packaging/flatpak/README.md).
#
# Steps:
#   1. tools/ci/l4/flatpak-manifest-check.py: the permissions are the approved ones and every
#      download is pinned.
#   2. flatpak-builder into a local repository (target/flatpak, or FLATPAK_WORK), the runtime and
#      SDK from Flathub when they are not installed yet.
#   3. flatpak build-bundle: <out-dir>/Redactor_<version>_x86_64.flatpak, which names Flathub as
#      the place to get the runtime from.
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)

app_id=io.github.its30na.redactor
branch=stable
flathub=https://dl.flathub.org/repo/flathub.flatpakrepo

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <out-dir>" >&2
  exit 2
fi

# flatpak-builder talks to a session bus; a container has none, so bring a private one.
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ] && [ -z "${BUNDLE_FLATPAK_BUS:-}" ] &&
  command -v dbus-run-session >/dev/null 2>&1; then
  BUNDLE_FLATPAK_BUS=1 exec dbus-run-session -- "$0" "$@"
fi

for tool in flatpak flatpak-builder python3; do
  command -v "$tool" >/dev/null || {
    echo "bundle-flatpak: $tool is missing" >&2
    exit 1
  }
done
mkdir -p "$1"
out=$(cd "$1" && pwd)
version=$("$repo/tools/ci/l4/lib.sh" app-version)
manifest=$repo/packaging/flatpak/$app_id.json
work=${FLATPAK_WORK:-$repo/target/flatpak}
mkdir -p "$work"
bundle=$out/Redactor_${version}_x86_64.flatpak

echo "::group::bundle-flatpak: manifest check"
python3 -I "$repo/tools/ci/l4/flatpak-manifest-check.py" "$manifest"
# The metainfo's newest release is the version flatpak shows; it must be this one.
grep -q "<release version=\"$version\"" "$repo/packaging/flatpak/$app_id.metainfo.xml" || {
  echo "bundle-flatpak: the metainfo has no <release version=\"$version\">" >&2
  exit 1
}
echo "::endgroup::"

# As root (the CI container) the system installation, else the user's.
scope=--system
[ "$(id -u)" -eq 0 ] || scope=--user

echo "::group::bundle-flatpak: flatpak-builder"
flatpak remote-add "$scope" --if-not-exists flathub "$flathub"
started=$(date +%s)
flatpak-builder "$scope" \
  --repo="$work/repo" \
  --state-dir="$work/state" \
  --disable-rofiles-fuse \
  --install-deps-from=flathub \
  --default-branch="$branch" \
  --arch=x86_64 \
  --force-clean \
  "$work/build" "$manifest"
echo "bundle-flatpak: flatpak-builder took $(($(date +%s) - started)) s"
echo "::endgroup::"

echo "::group::bundle-flatpak: build-bundle"
flatpak build-bundle --runtime-repo="$flathub" --arch=x86_64 \
  "$work/repo" "$bundle" "$app_id" "$branch"
echo "::endgroup::"

echo "bundle-flatpak: done, in $out:"
ls -l "$bundle"
