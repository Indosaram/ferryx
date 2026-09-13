@echo off
rem Gated recipe only: coordinator must approve/fingerprint source and InteractiveToken task first.
setlocal
set QA_ROOT=C:\Users\sook\ferryx-qa-dag-st01a099f8
if not exist "%QA_ROOT%\approved-source.json" exit /b 20
if not exist "%QA_ROOT%\orca-lite\package.json" exit /b 21
if not exist "%QA_ROOT%\evidence" exit /b 22
set FERRYX_RUNTIME_DIR=%QA_ROOT%\runtime
set FERRYX_SESSION_DIR=%QA_ROOT%\session
set APPDATA=%QA_ROOT%\appdata
set LOCALAPPDATA=%QA_ROOT%\localappdata
set WEBVIEW2_USER_DATA_FOLDER=%QA_ROOT%\webview2
set WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-address=127.0.0.1 --remote-debugging-port=9224
cd /d "%QA_ROOT%\orca-lite"
if errorlevel 1 exit /b 23
call bun tauri dev > "%QA_ROOT%\evidence\dev.log" 2>&1
exit /b %ERRORLEVEL%
