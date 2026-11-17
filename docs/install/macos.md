# Install Redactor on a Mac

> **Preview.** The Mac app works and is tested by machines on macOS 15 and 26, but it is not released yet and
> few people have used it by hand. Tell us what you find.

About 5 minutes. Needs macOS 13 or later.

## 1. Which Mac do you have?

Click the **Apple menu** (top left) > **About This Mac**.

- It says **Chip: Apple M1, M2, M3…** → you need the **Apple silicon** file (`Redactor-arm64.zip`).
- It says **Processor: Intel** → you need the **Intel** file (`Redactor-x86_64.zip`).

## 2. Get the app

Until the release, the Mac app is built by GitHub for every change and kept for 7 days:

1. Sign in to GitHub and open the repository's [**Actions**](https://github.com/its30NA/redactor/actions/workflows/ci.yml?query=branch%3Aport)
   page, then click the newest run with a green tick.
2. At the bottom, under **Artifacts**, click **redactor-app-macos-latest** (Apple silicon) or
   **redactor-app-macos-15-intel** (Intel). This downloads a zip.
3. Double-click it in **Downloads** until you see the zip named in step 1, and double-click that too. You now
   have **Redactor**.

No GitHub account? Someone who has one can download it and send you the zip (AirDrop or a USB stick).

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
Terminal can run `xattr -dr com.apple.quarantine /Applications/Redactor.app` once, then open it again.

## 4. Try it

1. Copy some text with a made-up password in it, for example `password: Plum-tree47`.
2. Click into the left pane and press **Cmd+V**. The middle pane shows the same text with the password
   replaced by a placeholder.
3. Click the copy icon above the middle pane (**Copy safe text**), or press **Shift+Cmd+C**, and paste the safe
   text wherever you wanted to send it.
4. The shortcut **Control+Option+R** cleans whatever is on the clipboard, from any app, without opening the
   window. Newer macOS versions may ask once whether Redactor may paste from other apps: click **Allow**.

Closing the window keeps Redactor in the menu bar (top right). Click its icon for the menu; **Quit** or
**Cmd+Q** ends it. Clicking Redactor in the Dock brings the window back.

Copies from password managers are left alone, and locking the Mac or putting it to sleep empties both panes.

## Remove it

1. Quit Redactor, then drag it from **Applications** to the **Trash** (called **Bin** on some Macs).
2. Your settings: in Finder choose **Go** > **Go to Folder…**, type `~/Library/Application Support/`, and
   delete the **Redactor** folder.

[Back to the overview](../../README.md#download)
