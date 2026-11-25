#!/usr/bin/env bash
# Build the macOS disk image (one universal file for Apple silicon and Intel) the way a release
# does, and check it. CI runs it on every pull request and release.yml for the release, so a pull
# request proves the release's own build steps.
#
# Usage: tools/release/bundle-macos.sh <out-dir>
# Before: the UI is built (app/ui: npm ci && npm run build) and RUSTFLAGS are set by the caller
# (a release passes tools/release/remap-path-flags.sh). On macOS with Xcode's command line tools.
#
# Steps:
#   1. rustup adds both Mac targets to the pinned toolchain.
#   2. tauri build --target universal-apple-darwin --bundles dmg: both halves are built, joined
#      with lipo, the .app is signed ad hoc (bundle.macOS.signingIdentity "-") and put in a disk
#      image. --ci skips the Finder window styling, which needs a logged-in Finder.
#   3. The app binary holds both architectures (lipo -archs) and the signature verifies
#      (codesign --verify --deep --strict).
#   4. Redactor_<version>_universal.dmg into <out-dir>, then tools/ci/l4/contents.sh on it.
# tools/ci/l4/dmg.sh then installs the app from the disk image and starts it.
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <out-dir>" >&2
  exit 2
fi
if [ "$(uname -s)" != Darwin ]; then
  echo "bundle-macos: this needs macOS" >&2
  exit 2
fi
for tool in rustup lipo codesign hdiutil; do
  command -v "$tool" >/dev/null || {
    echo "bundle-macos: $tool is missing" >&2
    exit 1
  }
done
mkdir -p "$1"
out=$(cd "$1" && pwd)

[ -f "$repo/app/ui/dist/index.html" ] || {
  echo "bundle-macos: app/ui/dist is missing; build the UI first (npm ci && npm run build in app/ui)" >&2
  exit 1
}
version=$("$repo/tools/ci/l4/lib.sh" app-version)
target_dir=${CARGO_TARGET_DIR:-$repo/target}
case "$target_dir" in /*) ;; *) target_dir=$repo/$target_dir ;; esac
bundle=$target_dir/universal-apple-darwin/release/bundle
tauri=$repo/app/ui/node_modules/.bin/tauri

echo "::group::bundle-macos: rust targets"
# Inside the repository, so rust-toolchain.toml picks the toolchain the targets go to.
(cd "$repo" && rustup target add aarch64-apple-darwin x86_64-apple-darwin)
echo "::endgroup::"

echo "::group::bundle-macos: tauri build (universal dmg)"
started=$(date +%s)
(cd "$repo/app/src-tauri" && "$tauri" build --ci --target universal-apple-darwin --bundles dmg -- --locked)
echo "bundle-macos: tauri build took $(($(date +%s) - started)) s"
echo "::endgroup::"

app=$bundle/macos/Redactor.app
dmg=$bundle/dmg/Redactor_${version}_universal.dmg
for f in "$app/Contents/MacOS/Redactor" "$dmg"; do
  [ -f "$f" ] || {
    echo "bundle-macos: the build made no $f" >&2
    ls -R "$bundle" >&2 || true
    exit 1
  }
done

status=0
# lipo prints the architectures in the file's order; compare them sorted.
archs=$(lipo -archs "$app/Contents/MacOS/Redactor" | tr ' ' '\n' | sort | tr '\n' ' ' | sed 's/ $//')
if [ "$archs" = "arm64 x86_64" ]; then
  echo "PASS universal (arm64 x86_64)"
else
  echo "FAIL universal the app binary holds '$archs', expected arm64 and x86_64"
  status=1
fi
if codesign --verify --deep --strict "$app" 2>&1; then
  echo "PASS signature ($(codesign -dv "$app" 2>&1 | sed -n 's/^Signature=//p'))"
else
  echo "FAIL signature codesign --verify --deep --strict failed on $app"
  status=1
fi

cp -f "$dmg" "$out/"
echo "--- contents: $(basename "$dmg")"
"$repo/tools/ci/l4/contents.sh" "$out/$(basename "$dmg")" --expect-version "$version" || status=1

[ "$status" -eq 0 ] || {
  echo "bundle-macos: a check failed" >&2
  exit 1
}
echo "bundle-macos: done, in $out:"
ls -l "$out"
