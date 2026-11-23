# Install Redactor on a Mac

> **Preview.** The Mac app works and is tested by machines on macOS 15 and 26, but it is not released yet and
> few people have used it by hand. What you get here is an unattested test build: not a release download,
> not signed by an Apple developer account, with no checksum and no build attestation. Tell us what you find.

About 5 minutes. Needs macOS 13 or later.

## 1. Which Mac do you have?

Click the **Apple menu** (top left) > **About This Mac**.

- It says **Chip: Apple M1, M2, M3…** → you need the **Apple silicon** file (`Redactor-arm64.zip`).
- It says **Processor: Intel** → you need the **Intel** file (`Redactor-x86_64.zip`).

## 2. Get the app

Until the release, the Mac app is a test build that GitHub makes for every change and keeps for 7 days.
There is no checksum or attestation to check it against, so download it only from this repository's own
Actions page:

1. Sign in to GitHub and open the repository's [**Actions**](https://github.com/its30NA/redactor/actions/workflows/ci.yml?query=branch%3Aport)
   page, then click the newest run with a green tick.
2. At the bottom, under **Artifacts**, click **redactor-app-macos-latest** (Apple silicon) or
   **redactor-app-macos-15-intel** (Intel). This downloads a zip.
3. Double-click it in **Downloads** until you see the zip named in step 1, and double-click that too. You now
   have **Redactor**.

No GitHub account? Someone who has one can download it and send you the zip (AirDrop or a USB stick).
Only take it from someone you trust: nothing lets you check that the zip is the one GitHub built.

## 3. Install

1. Drag **Redactor** into your **Applications** folder.
2. Open **Applications** and double-click **Redactor**.
3. The Mac says **"Redactor" Not Opened** ("Apple could not verify…"). That is because Redactor is not
   signed by an Apple developer account yet, not because something is wrong with it. Click **Done**.
4. Open **System Settings** > **Privacy & Security**, scroll down to **Security**. Next to
   *"Redactor" was blocked…*, click **Open Anyway**, then confirm with your password or Touch ID.
5. Redactor opens. From now on it opens normally.

On macOS 13 or 14 there is a shortcut instead: right-click **Redactor** > **Open** > **Open**.

If the Mac says the app **"is damaged"**: that is the same check being stricter. Someone comfortable with the
Terminal can run `xattr -dr com.apple.quarantine /Applications/Redactor.app` once, then open it again. That
line switches off macOS's own check of the download, so run it only on a zip you took from the Actions
page yourself.

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
- **Some messages still use the Windows key names.** The welcome card and Settings show the shortcut as
  Control, Option and Command, but a message the app writes about a key it could not set still says Ctrl
  and Alt (for example "Ctrl+Alt+R").
- **No warning when another app has the same shortcut.** macOS does not tell Redactor. If the shortcut
  does nothing, pick another one in Settings.
- **The preview is an unattested test build, not a release download.** It is signed only ad hoc (not by
  an Apple developer account) and comes with no checksum and no build attestation, so you cannot check
  that the zip is the one GitHub built. Download it only from this repository's Actions page, or build it
  yourself. The
  [threat model](../THREAT_MODEL.md#macos-and-linux-preview-what-is-different) lists what else differs on
  a Mac.

## Remove it

1. Quit Redactor, then drag it from **Applications** to the **Trash** (called **Bin** on some Macs).
2. Your settings: in Finder choose **Go** > **Go to Folder…**, type `~/Library/Application Support/`, and
   delete the **Redactor** folder.

[Back to the overview](../../README.md#download)
