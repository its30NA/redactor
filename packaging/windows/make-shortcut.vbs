' Creates a clean "Redactor" desktop shortcut (custom name + icon, starts minimized)
' pointing at Redactor.bat. Run once with:  cscript //nologo make-shortcut.vbs
' Uses VBScript (no PowerShell execution-policy changes needed). Edit the paths below.
Set W = CreateObject("WScript.Shell")
desktop = W.SpecialFolders("Desktop")
Set S = W.CreateShortcut(desktop & "\Redactor.lnk")
S.TargetPath = desktop & "\Redactor.bat"
S.WorkingDirectory = desktop
' Use the bundled icon if you copied assets/redactor.ico to the Desktop; otherwise
' fall back to a stock Windows lock icon.
If W.CreateObject("Scripting.FileSystemObject").FileExists(desktop & "\redactor.ico") Then
  S.IconLocation = desktop & "\redactor.ico,0"
Else
  S.IconLocation = "C:\Windows\System32\shell32.dll,48"
End If
S.WindowStyle = 7                                       ' start minimized
S.Description = "Redactor - local, private text sanitizer"
S.Save
WScript.Echo "Created " & desktop & "\Redactor.lnk"
