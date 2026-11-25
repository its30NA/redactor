#!/usr/bin/env bash
# Build the Linux bundles (AppImage, .deb, .rpm) the way a release does, and check them. CI runs
# it on every pull request and release.yml for the release, so a pull request proves the
# release's own build steps.
#
# Usage: tools/release/bundle-linux.sh <out-dir>
# Before: the UI is built (app/ui: npm ci && npm run build) and RUSTFLAGS are set by the caller
# (a release passes tools/release/remap-path-flags.sh). On Ubuntu 22.04 x86_64 with the app's
# -dev packages (CONTRIBUTING.md), plus `file`.
#
# Steps:
#   1. tools/release/appimage-tools.sh: the AppImage tools, pinned and checked, into the cache
#      Tauri reads, and the AppImage runtime passed on with LDAI_RUNTIME_FILE.
#   2. tauri build --bundles appimage,deb,rpm.
#   3. tools/release/appimage-licenses.sh: the licenses of the libraries the AppImage bundles,
#      plus packaging/appimage/APPIMAGE-PARTS.txt for the runtime, AppRun and the GTK hook.
#   4. The AppImage again (only the bundling; cargo's build is up to date) with those licenses
#      at usr/share/doc/redactor/bundled-libraries.
#   5. The three files into <out-dir> under Tauri's names, then tools/ci/l4/contents.sh on each
#      (the .rpm where rpm2cpio and cpio exist).
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <out-dir>" >&2
  exit 2
fi
mkdir -p "$1"
out=$(cd "$1" && pwd)

[ -f "$repo/app/ui/dist/index.html" ] || {
  echo "bundle-linux: app/ui/dist is missing; build the UI first (npm ci && npm run build in app/ui)" >&2
  exit 1
}
version=$("$repo/tools/ci/l4/lib.sh" app-version)
target_dir=${CARGO_TARGET_DIR:-$repo/target}
case "$target_dir" in /*) ;; *) target_dir=$repo/$target_dir ;; esac
bundle=$target_dir/release/bundle
tauri=$repo/app/ui/node_modules/.bin/tauri

echo "::group::bundle-linux: AppImage tools"
LDAI_RUNTIME_FILE=$("$repo/tools/release/appimage-tools.sh" | tail -n 1)
export LDAI_RUNTIME_FILE
echo "::endgroup::"

echo "::group::bundle-linux: tauri build (appimage, deb, rpm)"
(cd "$repo/app/src-tauri" && "$tauri" build --ci --bundles appimage,deb,rpm -- --locked)
echo "::endgroup::"

appimage=$bundle/appimage/Redactor_${version}_amd64.AppImage
deb=$bundle/deb/Redactor_${version}_amd64.deb
rpm=$bundle/rpm/Redactor-${version}-1.x86_64.rpm
for f in "$appimage" "$deb" "$rpm"; do
  [ -f "$f" ] || {
    echo "bundle-linux: the build made no $f" >&2
    exit 1
  }
done
before=$(stat -c %s "$appimage")

echo "::group::bundle-linux: licenses of the AppImage's libraries"
licenses=$target_dir/appimage-licenses
"$repo/tools/release/appimage-licenses.sh" "$appimage" "$licenses"
cp -f "$repo/packaging/appimage/APPIMAGE-PARTS.txt" "$licenses/"
echo "::endgroup::"

echo "::group::bundle-linux: the AppImage again, with the licenses"
config=$(printf '{"bundle":{"linux":{"appimage":{"files":{"/usr/share/doc/redactor/bundled-libraries":"%s"}}}}}' "$licenses")
(cd "$repo/app/src-tauri" && "$tauri" build --ci --bundles appimage --config "$config" -- --locked)
echo "::endgroup::"
after=$(stat -c %s "$appimage")
echo "bundle-linux: AppImage $before bytes without the licenses, $after with them"

cp -f "$appimage" "$deb" "$rpm" "$out/"

status=0
checks=("$out/$(basename "$deb")" "$out/$(basename "$appimage")")
if command -v rpm2cpio >/dev/null && command -v cpio >/dev/null; then
  checks+=("$out/$(basename "$rpm")")
else
  echo "bundle-linux: no rpm2cpio/cpio here: the .rpm is checked in the L4 job (fedora)" >&2
fi
for f in "${checks[@]}"; do
  echo "--- contents: $(basename "$f")"
  "$repo/tools/ci/l4/contents.sh" "$f" --expect-version "$version" || status=1
done
[ "$status" -eq 0 ] || {
  echo "bundle-linux: a contents check failed" >&2
  exit 1
}
echo "bundle-linux: done, in $out:"
ls -l "$out"
