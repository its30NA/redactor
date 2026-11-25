# Install Redactor on Linux

> **Preview.** macOS and Linux are in preview: machine-tested, little hand use yet. The Linux app works on
> X11 and Wayland desktops. Every download is installed and started on clean systems by CI, but none of
> the four packages has been used by hand on a real desktop yet. On Wayland, what works depends on the
> desktop: see below. The [threat model](../THREAT_MODEL.md#macos-and-linux-preview-what-is-different)
> lists what differs from Windows.

About 5 minutes. For a 64-bit PC (x86_64) with glibc 2.35 or newer: Ubuntu 22.04, Debian 12, Fedora 42 or
later. There is no Linux download for ARM yet.

## 1. Pick a download

All four are on the [latest release](https://github.com/its30NA/redactor/releases/latest) under
**Assets**, and all four hold the same app. `<version>` is the release number, such as `2.1.1`.

| File | Choose it when |
|---|---|
| `Redactor_<version>_amd64.deb` | Ubuntu, Debian and the distributions based on them. Your package manager installs it, and the system's updates keep its libraries current. |
| `Redactor-<version>-1.x86_64.rpm` | Fedora, for the same reasons. Other rpm distributions may work but are not tested. |
| `Redactor_<version>_x86_64.flatpak` | Any distribution with Flatpak, or when you want the app in a sandbox. Its libraries come from the GNOME runtime on Flathub. |
| `Redactor_<version>_amd64.AppImage` | One file that runs without installing. It carries its own libraries, which are updated only with a new Redactor release, so prefer one of the others where you can. |

## 2. Verify

Download `SHA256SUMS` too, then, in the folder with both files:

```bash
sha256sum --check --ignore-missing SHA256SUMS     # the line for your file must say OK
gh attestation verify <file> --repo its30NA/redactor
```

The second line needs the [GitHub CLI](https://cli.github.com/), signed in once with `gh auth login`.
The [README](../../README.md#verify-the-download) says what each check proves. Do this before you
install: the packages are not signed with a GPG key, so your package manager does not check where they
came from.

## 3. Install

**`.deb`**

```bash
sudo apt install ./Redactor_<version>_amd64.deb
```

**`.rpm`**

```bash
sudo dnf install ./Redactor-<version>-1.x86_64.rpm
```

Both put **Redactor** in your applications menu. From a terminal it is `Redactor` (capital R).

**Flatpak**

```bash
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user Redactor_<version>_x86_64.flatpak
flatpak run io.github.its30na.redactor
```

The first line is needed only if Flathub is not set up yet: Flatpak fetches the GNOME runtime from it
(about 1 GB the first time). The app then shows in your applications menu.

**AppImage**

```bash
chmod +x Redactor_<version>_amd64.AppImage
./Redactor_<version>_amd64.AppImage
```

Keep the file where it is: the hotkey command below names it by its path. The AppImage mounts itself with
FUSE; if it does not start and the message mentions FUSE, install your distribution's `fuse3` package, or
run it as `./Redactor_<version>_amd64.AppImage --appimage-extract-and-run`. It adds no menu entry.

**Check:** `Redactor --version` (or the AppImage file, or `flatpak run io.github.its30na.redactor
--version`) prints `Redactor <version>`.

**Try it:** copy some text with a made-up password in it, for example `password: Plum-tree47`, click
into the left pane and press **Ctrl+V**. The middle pane shows the same text with the password replaced
by a placeholder. **Ctrl+Shift+C** copies the safe text.

## Which desktop are you on?

Open a terminal and type:

```bash
echo $WAYLAND_DISPLAY
```

- **It prints something** (such as `wayland-0`) → you are on **Wayland**, also if `DISPLAY` is set too.
  Most current Ubuntu and Fedora desktops are. The app runs here as a normal window. Read
  [If you are on Wayland](#if-you-are-on-wayland).
- **It prints nothing** → type `echo $DISPLAY`. If that prints something (such as `:0`), you are on **X11**
  and the app runs here, with the hotkey **Ctrl+Alt+R** working from any window.

This is the rule the app itself uses. `echo $XDG_SESSION_TYPE` gives a hint too (`x11` or `wayland`), but
it can be empty: on WSLg it prints nothing.

## If you are on Wayland

Redactor runs as a native Wayland window, not through the X11 compatibility layer. A session counts as
Wayland when `WAYLAND_DISPLAY` is set, also if `DISPLAY` is set too.

### What works on which desktop

| Desktop | Reading and scrubbing the clipboard | The hotkey |
|---|---|---|
| **sway and other wlroots desktops (Hyprland: expected, not tested), KDE Plasma** | Works in the background: you can press the hotkey from any window. | See below. |
| **GNOME, Weston, WSLg (Windows)** | Works only **while the Redactor window is in front**. The hotkey brings the window forward first, then scrubs (on WSLg it scrubs at once, unless the window is minimised). | See below. |

These desktops do not let an app read the clipboard in the background, which is why the second row is
different. Redactor does not work around that: it asks for its window to come forward instead.

On sway the app is tested by machines; on KDE Plasma 6 only its clipboard code is. GNOME is not (the machines have no full GNOME session), and
WSLg was tried by hand (see Known limits).

### The hotkey

Which of these you get depends on the desktop. Settings shows which one.

1. **A desktop with a shortcut service (GNOME 48 or newer, KDE Plasma 6).** Redactor asks the desktop for
   the key in Settings. The first time, the desktop shows a dialog: click once to allow it. The desktop may
   change the key or let you pick another in its own keyboard settings, so the key in Settings may not be
   the one that works. Settings says so. This path is not tested by machines against the real GNOME or KDE
   dialogs; the conversation with the service is tested against a stand-in.

   If the desktop later drops the shortcut (for example when its shortcut service restarts), Redactor tells
   you once, with a desktop notification and a notice in the window ("Hotkey stopped working"), and
   Settings shows the key as not active. Set the key again in Settings. It does not come back by itself.
2. **Everywhere else (sway, older GNOME or KDE, WSLg; Hyprland is not tested and may offer the shortcut service of 1 instead).** The desktop does not let an app set a
   hotkey. Settings shows the command to run instead, with a **Copy** button. It depends on how you
   installed Redactor, and is in quotes when a path has spaces or other special characters:

   | Installed from | The command Settings shows |
   |---|---|
   | `.deb` or `.rpm` | `/usr/bin/Redactor --scrub-clipboard` |
   | Flatpak | `flatpak run io.github.its30na.redactor --scrub-clipboard` |
   | AppImage | the full path of the AppImage file, such as `/home/you/Apps/Redactor_<version>_amd64.AppImage --scrub-clipboard` |
   | Built from source | the full path of the program you built, such as `/home/you/redactor/target/release/Redactor --scrub-clipboard` |

   Use the **Copy** button rather than typing it: the program is called `Redactor` (capital R), so a
   plain `redactor` does nothing. Add a shortcut in the desktop's own keyboard settings that runs the
   copied command. For sway, put this in `~/.config/sway/config` (change the keys to your taste, paste
   the command after `exec`) and reload:

   ```text
   bindsym $mod+Shift+r exec /usr/bin/Redactor --scrub-clipboard
   ```

   For Hyprland, in `~/.config/hypr/hyprland.conf`:

   ```text
   bind = SUPER SHIFT, R, exec, /usr/bin/Redactor --scrub-clipboard
   ```

   The command asks the running Redactor to scrub the clipboard, then returns at once. If Redactor is not
   running, the command starts it. Where the clipboard works in the background, it scrubs at once, and the
   window shows the result once it is up. Where the clipboard needs the window in front, it waits for the
   window first (see the next paragraph).

   Where the clipboard works only with the window in front, the command also brings the window forward and
   scrubs when it is in front. It waits up to 30 seconds; time the computer spends asleep counts. If the
   window is not in front by then, Redactor gives up and tells you with a desktop notification: "Not
   scrubbed. Redactor's window did not come to the front in time." The window says "Not scrubbed" too, with
   a **Scrub now** button, at once if it is on screen and again when you next bring it forward (within 10
   minutes). The clipboard is left as it was. GNOME may refuse to pull a window forward; the scrub then
   happens when you click the window.

With **Strip invisible characters** on (the default), the hotkey also removes zero-width, bidi and tag
characters from a copy with no secret in it, and says "Hidden characters removed".

### Known limits on Wayland

- **After you quit Redactor, the scrubbed text may be gone from the clipboard.** On sway and other desktops
  without a clipboard manager, the clipboard belongs to the app that copied: when Redactor quits, the text it
  put there goes with it. On KDE Plasma this is not tested; Plasma's clipboard manager may keep a copy. Paste
  before you quit, or run a clipboard manager. <!-- PENDING hand test: whether text Redactor put on the
  clipboard survives quitting on WSLg (hand test H). Write what it does once it has been tried. -->
- **A copy made in the very same instant can lose.** If you copy something just before Redactor puts the
  scrubbed text on the clipboard, your copy can be overwritten. Wayland has no way to lock the clipboard for
  that moment. The window is very small (one wake-up of Redactor's clipboard thread). A test tries the race
  2000 times; on a local sway at most about 2 in 100 rounds ended with our text over the newer copy.
- **Where the window must be in front,** a copy over 20 MiB is refused, but the window's toolkit has to read
  all of it first, so that takes a moment. The toolkit also keeps its own copy of the text for as long as the
  window lives; Redactor cannot wipe that one.
- **WSLg shows no desktop notifications and does not bring the window forward.** The command does scrub
  (tried by hand on WSLg), but the result shows only as a notice inside Redactor's window. If the window is
  behind other windows, you see nothing. Check by pasting.
- **WSLg with the window minimised: the scrub may not happen, and you learn it only in the window.** The
  command waits 30 seconds for the window to be in front. Then the window says "Not scrubbed", with a
  **Scrub now** button, when you bring it back within 10 minutes. Keep the window open (not minimised) when
  you use the command on WSLg.
  <!-- PENDING hand test: WSLg, window minimised (hand test D/D2). Two runs left the clipboard raw; the
  cause is probably the 30 s wait but is not proven, and a fix may come. Reword when settled. -->
- **Clicking away right after the command.** If you click another window just after pressing the hotkey,
  the window may not stay in front. <!-- PENDING hand test: who wins, the click or Redactor (hand test G).
  Write what happened. The code never scrubs an old copy and never scrubs twice for one press. -->
- **A shortcut the desktop refuses.** If the shortcut service refuses the key you pick in Settings, no
  hotkey is set at all. Settings keeps showing the old key, marked "Not active: pick another key.", and
  the reason ("Your desktop did not accept ... No hotkey is set now; pick another in Settings.") stays
  under it until you try again.
- **No clipboard history to delete** on Linux, same as on X11. A clipboard manager's own history is out of
  Redactor's reach.

## Known limits on every Linux desktop

- **Wipe when the screen locks needs a lock signal.** Redactor wipes when logind says the session locks or
  the computer goes to sleep, or when a screen saver (freedesktop, GNOME, Cinnamon, MATE, Xfce) says it
  turned on. A locker that says nothing goes unseen: `swaylock` or `i3lock` started directly, and locking
  Windows while Redactor runs in WSLg. Where no lock signal can be found at all, Settings says so under
  **Wipe when the screen locks**: "No lock signal found on this desktop: a screen lock does not wipe.
  Suspend and the idle timer still do." On sway, lock through logind so the lock is announced: bind your
  lock key to `loginctl lock-session` and let swayidle start the locker:

  ```text
  bindsym $mod+Escape exec loginctl lock-session
  exec swayidle -w lock 'swaylock -f'
  ```

  The idle timer (**Wipe when idle**) is the backstop either way.
- **A hotkey that could not be set.** If another app holds the key at start, or the desktop refuses it,
  the window says "Hotkey not active" and Settings shows "Not active: pick another key."

- **The AppImage's libraries are not updated by your system.** It carries WebKitGTK and about 180 other
  libraries from Ubuntu 22.04, which get security fixes only with the next Redactor release. The `.deb`,
  the `.rpm` and the Flatpak do not have this limit.
- **The Flatpak on Wayland may need the window in front.** Some desktops hide the background clipboard
  from sandboxed apps. Redactor then works as on GNOME (see the table above). Not tested on a real
  desktop.
- **The Flatpak on X11 is not separated from your other apps.** X11 lets every app read every other app's
  clipboard and keys, and the sandbox cannot change that.

## Your files

- **Settings:** `~/.config/redactor/settings.json` (or `$XDG_CONFIG_HOME/redactor/`); in the Flatpak,
  `~/.var/app/io.github.its30na.redactor/config/redactor/settings.json`. It holds your settings and the
  always-redact word list in plain text, never scanned text. The folder is readable by you alone (0700)
  and the file too (0600); a file from an older build is tightened at the next settings change. If the
  file cannot be read, Redactor starts on the defaults, keeps the old file as `settings.json.bad` next to
  it (so you can copy your word list back out) and says so in the window.
- **The web view's folder:** `~/.local/share/io.github.its30na.redactor` (in the Flatpak, under
  `~/.var/app/io.github.its30na.redactor/data/`). WebKitGTK creates it with `hsts-storage.sqlite`, a few
  files named `salt` and empty cache folders. It holds no page data.

## Remove it

Quit Redactor first (tray icon > **Quit**).

| Installed from | Remove the app | What stays behind |
|---|---|---|
| `.deb` | `sudo apt remove redactor` | `~/.config/redactor` and `~/.local/share/io.github.its30na.redactor`: delete both. |
| `.rpm` | `sudo dnf remove redactor` | The same two folders. |
| Flatpak | `flatpak uninstall --user --delete-data io.github.its30na.redactor` | Nothing of Redactor's: `--delete-data` removes `~/.var/app/io.github.its30na.redactor`. The GNOME runtime stays for other apps; `flatpak uninstall --user --unused` removes it when nothing needs it. |
| AppImage | Delete the file. | The same two folders as the `.deb`. |

Also remove a keyboard shortcut you added for `--scrub-clipboard`.

## The `scrub` command-line tool

`scrub` does the same redaction in a terminal. Install it as the
[README describes](../../README.md#install-scrub).

## Build it from source

The app needs WebKitGTK 4.1 and, to hide in the tray, a desktop that shows tray icons. The steps and the
packages are in [CONTRIBUTING: Build the app](../../CONTRIBUTING.md#build-the-app). The result is
`target/release/Redactor`. To remove it, delete the program and the two folders under **Your files**.

[Back to the overview](../../README.md#download)
