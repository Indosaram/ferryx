@echo off
set DIR=%~dp0
where bun >nul 2>nul
if %ERRORLEVEL% equ 0 (
  bun run "%DIR%fixture-pty-child.mjs" %*
  exit /b %ERRORLEVEL%
)
where node >nul 2>nul
if %ERRORLEVEL% equ 0 (
  node "%DIR%fixture-pty-child.mjs" %*
  exit /b %ERRORLEVEL%
)
echo Neither bun nor node found on PATH >&2
exit /b 1
