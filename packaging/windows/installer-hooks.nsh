; Tauri NSIS installer hooks (bundle.windows.nsis.installerHooks).
;
; Uninstalling Ferryx removes the daemon's scheduled tasks (every runtime generation and the
; legacy Ferryx\Daemon task) through `ferryx-cli service uninstall`, which owns the task naming.
; A running daemon is never stopped here (no --force): deleting a task does not end the watchdog it
; already runs, so open terminals survive until the daemon exits, and nothing starts it at the next
; logon. An update (/UPDATE) keeps the tasks, since the new version re-registers its own generation.
; The uninstaller runs as the installing user (installMode currentUser), so the per-user tasks are
; in scope.
; Right after this hook the Tauri template runs CheckIfAppIsRunning "ferryx.exe", which kills every
; process of the user with that image name. The staged daemon is therefore ferryx-daemon.exe
; (runtime_stage.rs WINDOWS_DAEMON_FILE_NAME); a daemon still running from a generation an older
; build staged as ferryx.exe is killed by that check until it is replaced.

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
  ${AndIf} ${FileExists} "$INSTDIR\ferryx-cli.exe"
    DetailPrint "Removing the Ferryx daemon scheduled tasks"
    nsExec::ExecToLog '"$INSTDIR\ferryx-cli.exe" service uninstall'
    Pop $0
    DetailPrint "ferryx-cli service uninstall exited with $0"
  ${EndIf}
!macroend
