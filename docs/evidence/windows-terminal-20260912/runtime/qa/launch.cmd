@echo off
rem Ferryx QA runtime launch - task st_01a0958a - root command: bun tauri dev (debug)
rem Isolation: daemon runtime + session state redirected into the QA tree; installed app untouched.
setlocal
set QA_ROOT=C:\Users\sook\ferryx-qa-rt-st01a0958a
set FERRYX_RUNTIME_DIR=%QA_ROOT%\runtime
set FERRYX_SESSION_DIR=%QA_ROOT%\session
set WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223
cd /d %QA_ROOT%\orca-lite
echo [launch] start %DATE% %TIME% > "%QA_ROOT%\logs\dev-stdout.log"
echo [launch] FERRYX_RUNTIME_DIR=%FERRYX_RUNTIME_DIR% >> "%QA_ROOT%\logs\dev-stdout.log"
echo [launch] FERRYX_SESSION_DIR=%FERRYX_SESSION_DIR% >> "%QA_ROOT%\logs\dev-stdout.log"
echo [launch] WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=%WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS% >> "%QA_ROOT%\logs\dev-stdout.log"
echo [launch] invoking root command: bun tauri dev >> "%QA_ROOT%\logs\dev-stdout.log"
call bun tauri dev >> "%QA_ROOT%\logs\dev-stdout.log" 2>&1
echo [launch] exited code %ERRORLEVEL% at %DATE% %TIME% >> "%QA_ROOT%\logs\dev-stdout.log"
endlocal
