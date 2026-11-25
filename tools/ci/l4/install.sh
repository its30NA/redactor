#!/usr/bin/env bash
# The L4 install step: in a clean container (run as root), installs the test tools and then the
# package, the way a user would, and prints the command that starts the app on its last line
# (the only line on stdout; everything else goes to stderr).
#
# Usage: tools/ci/l4/install.sh <deb|rpm|appimage|flatpak> <file>
#   deb       apt-get install ./<file>     (Debian, Ubuntu)        -> /usr/bin/Redactor
#   rpm       dnf install ./<file>         (Fedora)                -> /usr/bin/Redactor
#   appimage  chmod +x; no FUSE in a container, so the AppImage unpacks itself each start
#             -> <file> --appimage-extract-and-run
#   flatpak   flatpak install --user, with the Flathub remote for the runtime (Fedora)
#             -> env FLATPAK_USER_DIR=<the user installation> flatpak run io.github.its30na.redactor
#             (start.sh gives the app a HOME and XDG_DATA_HOME of its own, where flatpak would
#             otherwise look for the user installation and find none)
#
# The test tools are those of tools/ci/l4/start.sh and contents.sh: Xvfb, xdotool, a session bus
# (dbus-run-session), xprop for a look by hand; on Fedora also setsid, find, cmp, and cpio for
# the .rpm's contents (rpm2cpio comes with rpm). An AppImage carries most of its libraries but
# not those every desktop has (the AppImage exclude list: GL/EGL, X11 and xcb, fontconfig,
# freetype, harfbuzz, fribidi, gbm, wayland-client, expat), so for it those are installed as a
# desktop would have them, plus a font.
set -euo pipefail

app_id=io.github.its30na.redactor

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <deb|rpm|appimage|flatpak> <file>" >&2
  exit 2
fi
format=$1
file=$(realpath -- "$2")
[ -f "$file" ] || {
  echo "install.sh: no such file: $2" >&2
  exit 2
}

if command -v apt-get >/dev/null 2>&1; then
  pm=apt
elif command -v dnf >/dev/null 2>&1; then
  pm=dnf
else
  echo "install.sh: neither apt-get nor dnf found" >&2
  exit 2
fi

apt_tools=(xvfb xdotool dbus dbus-x11 x11-utils)
apt_desktop=(libgl1 libegl1 libgl1-mesa-dri libgbm1 libfontconfig1 libfreetype6 libfribidi0
  libharfbuzz0b libwayland-client0 libx11-6 libx11-xcb1 libxcb1 libexpat1 fonts-dejavu-core)
dnf_tools=(xorg-x11-server-Xvfb xdotool dbus-daemon dbus-tools xprop util-linux-core findutils
  diffutils cpio)
dnf_desktop=(libglvnd-glx libglvnd-egl mesa-dri-drivers mesa-libgbm fontconfig freetype fribidi
  harfbuzz libwayland-client libX11 libX11-xcb libxcb expat dejavu-sans-fonts)

# install <packages...>: from the distribution, as root, without questions.
install() {
  case "$pm" in
    apt)
      DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$@" >&2
      ;;
    dnf)
      dnf install -y --setopt=install_weak_deps=False "$@" >&2
      ;;
  esac
}

if [ "$pm" = apt ]; then
  apt-get update >&2
  install "${apt_tools[@]}"
else
  install "${dnf_tools[@]}"
fi

case "$format:$pm" in
  deb:apt)
    # Recommends as a user gets them: apt's default.
    DEBIAN_FRONTEND=noninteractive apt-get install -y "$file" >&2
    echo /usr/bin/Redactor
    ;;
  rpm:dnf)
    dnf install -y "$file" >&2
    echo /usr/bin/Redactor
    ;;
  appimage:apt | appimage:dnf)
    if [ "$pm" = apt ]; then install "${apt_desktop[@]}"; else install "${dnf_desktop[@]}"; fi
    chmod +x "$file"
    echo "$file --appimage-extract-and-run"
    ;;
  flatpak:dnf | flatpak:apt)
    install flatpak ca-certificates
    flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo >&2
    flatpak install --user --noninteractive --assumeyes "$file" >&2
    user_dir=${FLATPAK_USER_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/flatpak}
    case "$user_dir" in
      *[[:space:]]*)
        echo "install.sh: the flatpak user folder has a space: $user_dir" >&2
        exit 2
        ;;
    esac
    echo "env FLATPAK_USER_DIR=$user_dir flatpak run $app_id"
    ;;
  *)
    echo "install.sh: $format is not installed with $pm" >&2
    exit 2
    ;;
esac
