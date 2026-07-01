@echo off
rem ===================================================================
rem  Redactor desktop launcher (Windows + WSL)
rem  Replace the two placeholders below, then drop this on your Desktop.
rem    __WSL_DISTRO__  e.g. Ubuntu   (see: wsl -l -q)
rem    __SCRUB_PATH__  absolute Linux path to the venv's scrub, e.g.
rem                    /home/you/code/redactor/.venv/bin/scrub
rem  See packaging/README.md for the one-time setup.
rem ===================================================================
title Redactor UI  -  keep this window open; close it to stop
echo.
echo     Redactor - local, private text sanitizer
echo     Opening http://localhost:8765 in your browser...
echo.
echo     Keep this window open while you use it.
echo     Close it (or press Ctrl-C) to stop the server.
echo.
rem Open the browser a few seconds from now, without blocking the server:
start "" /b cmd /c "timeout /t 3 >nul & start http://localhost:8765"
wsl.exe -d __WSL_DISTRO__ -e __SCRUB_PATH__ ui --no-browser
