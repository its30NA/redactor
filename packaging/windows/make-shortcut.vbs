' Creates a clean "Redactor" desktop shortcut (custom name + icon, starts minimized)
' pointing at Redactor.bat. Run once with:  cscript //nologo make-shortcut.vbs
' Uses VBScript (no PowerShell execution-policy changes needed). Edit the paths below.
Set W = CreateObject("WScript.Shell")
desktop = W.SpecialFolders("Desktop")
Set S = W.CreateShortcut(desktop & "\Redactor.lnk")
S.TargetPath = desktop & "\Redactor.bat"
S.WorkingDirectory = desktop
S.IconLocation = "C:\Windows\System32\shell32.dll,48"   ' a lock/keys icon
S.WindowStyle = 7                                       ' start minimized
S.Description = "Redactor - local, private text sanitizer"
S.Save
WScript.Echo "Created " & desktop & "\Redactor.lnk"
