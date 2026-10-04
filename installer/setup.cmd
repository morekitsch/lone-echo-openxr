@echo off
if exist "%~dp0setup.exe" (
  "%~dp0setup.exe" %*
  exit /b
)
where py >nul 2>nul
if %errorlevel% equ 0 (
  py -3 "%~dp0echo_setup.py" %*
) else (
  python "%~dp0echo_setup.py" %*
)
