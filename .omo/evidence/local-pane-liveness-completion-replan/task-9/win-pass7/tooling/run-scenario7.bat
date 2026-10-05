@echo off
setlocal
set BASE=C:\Users\sook\ferryx-pane-completion
set ROOT=%BASE%\source-21dea3c0
set T9=%BASE%\task9-42fba06f
set SC=%1
set BIN=%ROOT%\src-tauri\target\debug\ferryx.exe
set EV=%T9%\evidence\%SC%
set ISO=%T9%\runtime\%SC%
cd /d %ROOT%
node scripts\qa\pane-liveness.mjs --scenario %SC% --binary "%BIN%" --evidence-dir "%EV%" --isolation-root "%ISO%" > "%T9%\logs\bat-%SC%.out" 2> "%T9%\logs\bat-%SC%.err"
echo %ERRORLEVEL% > "%T9%\logs\bat-%SC%.exit"
echo BAT_DONE_%SC%
