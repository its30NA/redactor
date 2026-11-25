#!/usr/bin/env bash
# Put the tools Tauri's AppImage step runs into its cache, pinned and checked, before
# `tauri build --bundles appimage`.
#
# The bundler (tauri-cli 2.12.x) downloads AppRun, linuxdeploy and linuxdeploy-plugin-appimage
# into <cache>/tauri when they are missing and checks no hash; the plugin comes from a moving
# `continuous` release, and appimagetool inside it fetches the AppImage runtime from another
# `continuous` release at every build. This script downloads fixed releases instead, checks each
# against the sha256 below and puts them where the bundler finds them, so it downloads nothing.
# The runtime goes next to them; the caller passes it on with
#   LDAI_RUNTIME_FILE=<cache>/tauri/runtime-x86_64
# (tools/release/bundle-linux.sh does). A cached file with another hash is replaced.
#
# Usage: tools/release/appimage-tools.sh      (cache: ${XDG_CACHE_HOME:-$HOME/.cache}/tauri)
# Prints the runtime's path on its last line. x86_64 only.
set -euo pipefail

dir=${XDG_CACHE_HOME:-$HOME/.cache}/tauri
mkdir -p "$dir"

# <file in the cache> <sha256> <url>. Looked up 2026-11-25: the digests of AppRun, linuxdeploy
# and the runtime are the ones GitHub shows for the release assets; the plugin's release shows
# none, so its sha256 is the one measured on the first download.
tools=(
  "AppRun-x86_64 f30140a43a0a59e46db21bdefdf749b9e9f2c6946e92afabbacf98b8ae73fb4f https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64"
  "linuxdeploy-07333c6-x86_64.AppImage 36a2d7e274d12e1050d0e9ecfe11d339ed54720b2bec464c286d53f8b07f5c62 https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy-07333c6/linuxdeploy-x86_64.AppImage"
  "linuxdeploy-plugin-appimage.AppImage 992d502a248e14ab185448ddf6f6e7d25558cb84d4623c354c3af350c25fccb3 https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/1-alpha-20250213-1/linuxdeploy-plugin-appimage-x86_64.AppImage"
  "runtime-x86_64 2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64"
)

[ "$(uname -m)" = x86_64 ] || {
  echo "appimage-tools: x86_64 only, this is $(uname -m)" >&2
  exit 1
}

sha() { sha256sum "$1" | awk '{ print $1 }'; }

# cached_ok <file> <sha256>: the cached file is the pinned one. After its first use the bundler
# zeroes linuxdeploy's AppImage magic (bytes 8-10, "AI\x02") so desktop tools do not treat it as
# an app; that copy is checked with the magic put back.
cached_ok() {
  local file=$1 want=$2 copy
  [ -f "$file" ] || return 1
  [ "$(sha "$file")" = "$want" ] && return 0
  case "$file" in
    */linuxdeploy-*.AppImage)
      copy=$(mktemp "$dir/.check.XXXXXX")
      cp "$file" "$copy"
      printf 'AI\002' | dd of="$copy" bs=1 seek=8 conv=notrunc status=none
      if [ "$(sha "$copy")" = "$want" ]; then
        rm -f "$copy"
        return 0
      fi
      rm -f "$copy"
      ;;
  esac
  return 1
}

for entry in "${tools[@]}"; do
  read -r name want url <<<"$entry"
  target=$dir/$name
  if cached_ok "$target" "$want"; then
    echo "appimage-tools: $name ok (cached)" >&2
    continue
  fi
  part=$(mktemp "$dir/.download.XXXXXX")
  if ! curl -fsSL --retry 3 --proto '=https' --tlsv1.2 -o "$part" "$url"; then
    rm -f "$part"
    echo "appimage-tools: download failed: $url" >&2
    exit 1
  fi
  got=$(sha "$part")
  if [ "$got" != "$want" ]; then
    rm -f "$part"
    echo "appimage-tools: $name has sha256 $got, expected $want ($url)" >&2
    exit 1
  fi
  chmod 0755 "$part"
  mv -f "$part" "$target"
  echo "appimage-tools: $name ok (downloaded)" >&2
done

echo "$dir/runtime-x86_64"
