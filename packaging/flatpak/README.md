# Flatpak packaging

Redactor's Flatpak is built from source inside the GNOME SDK, with no network access during the
build.

## Files

| File | What it is |
|---|---|
| `io.github.its30na.redactor.json` | The flatpak-builder manifest: runtime `org.gnome.Platform//51`, the permissions, and the modules in build order. |
| `io.github.its30na.redactor.desktop` | The menu entry. `StartupWMClass=Redactor` is the window's class, so the desktop puts the window under this entry. |
| `io.github.its30na.redactor.metainfo.xml` | The AppStream metadata software centres show (summary, description, license, release). |
| `patches/` | Three small upstream fixes the tray libraries need to build in the SDK (from Flathub's shared modules; each keeps its author header). |

## Modules

1. `rust` and `node`: Rust 1.99 (the version in `rust-toolchain.toml`) and Node 22 from their
   official release archives, pinned by sha256. They are used for the build only and removed
   from the app afterwards (`cleanup`).
2. `intltool`: a build tool `libdbusmenu` needs; removed afterwards.
3. `libdbusmenu-gtk3`, `ayatana-ido`, `libayatana-indicator`, `libayatana-appindicator`: the tray
   icon's libraries, which the GNOME runtime does not have. Each installs its license files under
   `/app/share/licenses/<module>/`.
4. `redactor`: the app, built from this repository with `npm ci --offline` and
   `cargo build --locked --offline`. It installs the binary as `/app/bin/Redactor`, `LICENSE` and
   `THIRD_PARTY_NOTICES.md` under `/app/share/licenses/io.github.its30na.redactor/`, the icons,
   the desktop file and the metainfo.

## Permissions

The app gets only what it uses:

| Permission | Why |
|---|---|
| `--socket=wayland`, `--socket=fallback-x11`, `--share=ipc`, `--device=dri` | The window, the clipboard and GPU drawing. X11 only where there is no Wayland. |
| `--talk-name=org.kde.StatusNotifierWatcher` | The tray icon. |
| `--talk-name=org.freedesktop.Notifications` | The desktop notices (for example after the hotkey scrubs the clipboard). |
| `--talk-name=org.{freedesktop,gnome,cinnamon,mate,xfce}.ScreenSaver`, `--system-talk-name=org.freedesktop.login1` | Hearing that the screen locks, to wipe the panes and the remembered secrets. |

No network, no file system access (files you open or save go through the desktop's file dialog
portal), no sound. `tools/ci/l4/flatpak-manifest-check.py` fails CI if the list changes or a
download is not pinned:

```bash
python3 -I tools/ci/l4/flatpak-manifest-check.py packaging/flatpak/io.github.its30na.redactor.json
appstreamcli validate --no-net packaging/flatpak/io.github.its30na.redactor.metainfo.xml
```

## Build it yourself

Needs `flatpak` and `flatpak-builder`, and the GNOME 51 SDK from Flathub:

```bash
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user flathub org.gnome.Platform//51 org.gnome.Sdk//51
flatpak-builder --user --install --force-clean build-dir packaging/flatpak/io.github.its30na.redactor.json
flatpak run io.github.its30na.redactor
```
