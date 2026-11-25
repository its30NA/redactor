# Install Redactor on a Mac

> **Preview.** macOS and Linux are in preview: machine-tested, little hand use yet. The Mac app is tested
> by machines on macOS 15 and 26, and every disk image is opened and started on Apple silicon and Intel
> runners, but few people have used it by hand. It is not signed by an Apple developer account. Tell us
> what you find.

About 5 minutes. Needs macOS 13 or later.

## 1. Download

1. Open the [latest release](https://github.com/its30NA/redactor/releases/latest).
2. Under **Assets**, click **`Redactor_<version>_universal.dmg`** (the version number changes with each
   release). One file works on every Mac, Apple silicon and Intel. Your browser saves it to **Downloads**.

## 2. Verify

Redactor is not signed by an Apple developer account, so macOS cannot tell you who made it. Check the
file instead. Download **`SHA256SUMS`** from the same page, open **Terminal**, and run:

```bash
cd ~/Downloads
shasum -a 256 --check --ignore-missing SHA256SUMS    # the line for the dmg must say OK
gh attestation verify Redactor_<version>_universal.dmg --repo its30NA/redactor
```

The second line needs the [GitHub CLI](https://cli.github.com/), signed in once with `gh auth login`.
The [README](../../README.md#verify-the-download) says what each check proves.

## 3. Install

1. Double-click the dmg in **Downloads**. A window opens with **Redactor** and an **Applications** folder.
2. Drag **Redactor** onto **Applications**. Then eject the disk image (the eject button next to
   **Redactor** in the Finder sidebar) and delete the dmg if you like.
3. Open **Applications** and double-click **Redactor**.
4. The Mac says **"Redactor" Not Opened** ("Apple could not verify…"). That is because Redactor is not
   signed by an Apple developer account yet, not because something is wrong with it. Click **Done**.
5. Open **System Settings** > **Privacy & Security**, scroll down to **Security**. Next to
   *"Redactor" was blocked…*, click **Open Anyway**, then confirm with your password or Touch ID.
6. Redactor opens. From now on it opens normally.

On macOS 13 or 14 there is a shortcut instead: right-click **Redactor** > **Open** > **Open**.

If the Mac says the app **"is damaged"**: that is the same check being stricter. Someone comfortable with the
Terminal can run `xattr -dr com.apple.quarantine /Applications/Redactor.app` once, then open it again. That
line switches off macOS's own check of the download, so run it only on a copy whose dmg you verified in
step 2.

## 4. Try it

1. Copy some text with a made-up password in it, for example `password: Plum-tree47`.
2. Click into the left pane and press **Cmd+V**. The middle pane shows the same text with the password
   replaced by a placeholder.
3. Click the copy icon above the middle pane (**Copy safe text**), or press **Shift+Cmd+C**, and paste the safe
   text wherever you wanted to send it.
4. The shortcut **Control+Option+R** scrubs whatever is on the clipboard, from any app, without opening the
   window. Newer macOS versions may ask once whether Redactor may paste from other apps: click **Allow**.
   If it could not scrub, the window says **Clipboard not scrubbed** with a **Scrub now** button. The
   command palette (**Cmd+K**) has **Scrub the clipboard now** too.

Closing the window keeps Redactor in the menu bar (top right). Click its icon for the menu; **Quit** or
**Cmd+Q** ends it. Clicking Redactor in the Dock brings the window back.

Copies from password managers are left alone, and locking the Mac or putting it to sleep empties both panes.

## Known limits

- **No screen-capture shield.** Since macOS 15.4, screen capture ignores the setting that would hide the
  window, so Redactor has none on a Mac. Settings shows the row greyed out. Screen sharing shows the window.
- **No clipboard history cleaning.** macOS gives apps no way to remove one item from a clipboard history.
  The raw copy can stay in a clipboard tool, in Spotlight's clipboard history (macOS 26, off by default),
  and on your iPhone or other Mac through Universal Clipboard. Redactor cannot reach any of them.
- **"Paste from other apps".** Newer macOS versions may ask whether Redactor may paste from other apps.
  If you click **Don't Allow**, the shortcut cannot read the clipboard: Redactor should say **Clipboard not
  scrubbed** and leave the clipboard as it was. This has not been tried on a real Mac yet; if it says
  "Nothing to scrub" instead, the raw text is still there, so please tell us. To change your answer later,
  look for Redactor under **System Settings** > **Privacy & Security**.
- **No warning when another app has the same shortcut.** macOS does not tell Redactor. If the shortcut
  does nothing, pick another one in Settings.
- **Not signed by an Apple developer account, not notarized.** The app is signed only ad hoc, which is
  why macOS asks for **Open Anyway** the first time. The checksum and the attestation in step 2 are what
  show the file is the one GitHub built. The
  [threat model](../THREAT_MODEL.md#macos-and-linux-preview-what-is-different) lists what else differs on
  a Mac.
- **No automatic updates.** To update, download the new dmg and drag Redactor onto **Applications**
  again (choose **Replace**). Your settings stay.

## Remove it

1. Quit Redactor, then drag it from **Applications** to the **Trash** (called **Bin** on some Macs).
2. Your settings: in Finder choose **Go** > **Go to Folder…**, type `~/Library/Application Support/`, and
   delete the **Redactor** folder.

[Back to the overview](../../README.md#download)
