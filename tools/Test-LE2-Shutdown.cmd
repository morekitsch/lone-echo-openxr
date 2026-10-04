@echo off
setlocal
if not exist "%~dp0windows-python\python.exe" (
  echo Extract this helper beside setup.exe and the windows-python folder.
  pause
  exit /b 1
)
"%~dp0windows-python\python.exe" -I -X utf8 "%~dp0shutdown-check\test_windows_shutdown.py" --package "%~dp0."
set "test_result=%ERRORLEVEL%"
echo.
pause
exit /b %test_result%
