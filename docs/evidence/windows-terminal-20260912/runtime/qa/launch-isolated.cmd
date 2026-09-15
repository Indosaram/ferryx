rem Isolated launch v2 for st_01a0958a QA - root command: bun tauri dev (debug).
rem v2 adds app-settings + WebView profile isolation (NOT used on the first running
rem instance; recorded in the report). Daemon runtime isolation unchanged.
rem APPDATA redirect covers Tauri app_data_dir (%APPDATA%\com.ferryx.app).
rem WEBVIEW2_USER_DATA_FOLDER redirects the WebView2 profile out of %LOCALAPPDATA%.
setlocal
set QA_ROOT=C:\Users\sook\ferryx-qa-rt-st01a0958a
set FERRYX_RUNTIME_DIR=%QA_ROOT%\runtime
set FERRYX_SESSION_DIR=%QA_ROOT%\session
set APPDATA=%QA_ROOT%\appdata-roaming
set WEBVIEW2_USER_DATA_FOLDER=%QA_ROOT%\webview2
set WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223
if not exist "%QA_ROOT%\appdata-roaming" mkdir "%QA_ROOT%\appdata-roaming"
if not exist "%QA_ROOT%\webview2" mkdir "%QA_ROOT%\webview2"
cd /d %QA_ROOT%\orca-lite
echo [launch2] start %DATE% %TIME% > "%QA_ROOT%\logs\dev-stdout.log"
echo [launch2] FERRYX_RUNTIME_DIR=%FERRYX_RUNTIME_DIR% >> "%QA_ROOT%\logs\dev-stdout.log"
echo [launch2] FERRYX_SESSION_DIR=%FERRYX_SESSION_DIR% >> "%QA_ROOT%\logs\dev-stdout.log"
echo [launch2] APPDATA=%APPDATA% >> "%QA_ROOT%\logs\dev-stdout.log"
echo [launch2] WEBVIEW2_USER_DATA_FOLDER=%WEBVIEW2_USER_DATA_FOLDER% >> "%QA_ROOT%\logs\dev-stdout.log"
echo [launch2] WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=%WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS% >> "%QA_ROOT%\logs\dev-stdout.log"
echo [launch2] invoking root command: bun tauri dev >> "%QA_ROOT%\logs\dev-stdout.log"
call bun tauri dev >> "%QA_ROOT%\logs\dev-stdout.log" 2>&1
echo [launch2] exited code %ERRORLEVEL% at %DATE% %TIME% >> "%QA_ROOT%\logs\dev-stdout.log"
endlocal
