@echo off
setlocal
if not exist "%~dp0windows-python\python.exe" (
  echo Extract this helper beside setup.exe and the windows-python folder.
  pause
  exit /b 1
)
"%~dp0windows-python\python.exe" -I -X utf8 "%~dp0diagnose_windows_exit.py" --package "%~dp0."
set "diagnostic_result=%ERRORLEVEL%"
echo.
pause
exit /b %diagnostic_result%
