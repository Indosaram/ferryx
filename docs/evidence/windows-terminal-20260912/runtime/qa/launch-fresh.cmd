@echo off
setlocal
set QA_ROOT=C:\Users\sook\ferryx-qa-fresh-0912
set FERRYX_RUNTIME_DIR=%QA_ROOT%\runtime
set FERRYX_SESSION_DIR=%QA_ROOT%\session
set APPDATA=%QA_ROOT%\appdata
set WEBVIEW2_USER_DATA_FOLDER=%QA_ROOT%\webview2
set WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9233
cd /d %QA_ROOT%\orca-lite
call bun tauri dev > "%QA_ROOT%\logs\dev.log" 2>&1
exit /b %ERRORLEVEL%
