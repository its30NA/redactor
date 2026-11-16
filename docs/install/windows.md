# Install Redactor on Windows

About 3 minutes. No admin rights needed. Works on Windows 10 and 11.

## 1. Download

1. Open the [latest release](https://github.com/its30NA/redactor/releases/latest).
2. Under **Assets**, click **`Redactor_<version>_x64-setup.exe`** (the version number changes with each
   release). Your browser saves it to **Downloads**.

## 2. Install

1. Open **Downloads** and double-click the file you just downloaded.
2. Windows may show a blue box: **"Windows protected your PC"**. That is because Redactor is not signed yet,
   not because something is wrong with it. Click **More info**, then **Run anyway**.
   (Careful users check the download first: see
   [Verify the download](../../README.md#verify-the-download).)
3. Click through the installer (**Next**, **Install**, **Finish**). If your PC is missing Microsoft's
   WebView2 (rare on Windows 10 and 11), the installer downloads it from Microsoft first.
4. Open **Redactor** from the Start menu.

## 3. Try it

1. Copy some text with a made-up password in it, for example `password: Plum-tree47`.
2. Click into the left pane of Redactor and press **Ctrl+V**. The middle pane shows the same text with the
   password replaced by a placeholder.
3. Click the copy icon above the middle pane (**Copy safe text**), or press **Ctrl+Shift+C**, and paste the
   safe text wherever you wanted to send it.
4. The shortcut **Ctrl+Alt+R** cleans whatever is on the clipboard, from any app, without opening the window.

Closing the window keeps Redactor in the tray (bottom right, next to the clock). To quit, right-click the
tray icon and choose **Quit**. Click the tray icon to get the window back.

## Remove it

1. **Settings** > **Apps** > **Installed apps**, find **Redactor**, click **...** > **Uninstall**.
2. Your settings stay in `%APPDATA%\Redactor`. To remove them too, paste `%APPDATA%` into the address bar of
   File Explorer and delete the **Redactor** folder.

[Back to the overview](../../README.md#download)
