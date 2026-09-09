use super::*;
use base64::Engine as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(super) fn start(root: &Path, host: &str) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let executable = executable.to_str().ok_or("INVALID_REQUEST: executable path is not UTF-8")?;
    let root_text = root.to_str().ok_or("INVALID_REQUEST: helper path is not UTF-8")?;
    let quote = |arg: &str| {
        let mut result = String::from("\"");
        let mut slashes = 0;
        for ch in arg.chars() {
            if ch == '\\' {
                slashes += 1;
                continue;
            }
            result.extend(std::iter::repeat_n('\\', if ch == '"' { slashes * 2 + 1 } else { slashes }));
            result.push(ch);
            slashes = 0;
        }
        result.extend(std::iter::repeat_n('\\', slashes * 2));
        result.push('"');
        result
    };
    let command_line = [executable, "daemon", "--root", root_text, "--host-id", host]
        .map(quote).join(" ");
    let encoded = base64::engine::general_purpose::STANDARD.encode(command_line.as_bytes());
    // CIM owns this process outside the SSH job; console detachment alone does not.
    let script = format!(
        "$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';\
         $command=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded}'));\
         $startup=New-CimInstance -ClassName Win32_ProcessStartup -ClientOnly -Property @{{CreateFlags=[uint32]8}};\
         $r=Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{{CommandLine=$command;ProcessStartupInformation=$startup}};\
         $r|Select-Object ReturnValue,ProcessId|ConvertTo-Json -Compress"
    );
    let output = run(&script)?;
    let result: Value = serde_json::from_slice(&output).map_err(|e| format!("REMOTE_RUNTIME_SPAWN_FAILED: {e}"))?;
    if result["ReturnValue"].as_u64() != Some(0) {
        return Err(format!("REMOTE_RUNTIME_SPAWN_FAILED: Windows process creation returned {}", result["ReturnValue"]));
    }
    let pid = result["ProcessId"].as_u64().and_then(|pid| u32::try_from(pid).ok())
        .filter(|pid| *pid != 0).ok_or("REMOTE_RUNTIME_SPAWN_FAILED: missing Windows process ID")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if is_live(root, host) {
            println!("{}", json!({"event":"ready","protocol":1}));
            std::io::stdout().flush().map_err(|e| e.to_string())?;
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    run(&format!(
        "$ErrorActionPreference='Stop';$p=Get-CimInstance Win32_Process -Filter 'ProcessId={pid}';\
         if($p -and $p.CommandLine -eq [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded}')))\
         {{Stop-Process -Id {pid} -Force;Wait-Process -Id {pid} -Timeout 5 -ErrorAction SilentlyContinue}}"
    ))?;
    Err("REMOTE_RUNTIME_START_TIMEOUT: helper did not become ready".into())
}

fn run(script: &str) -> Result<Vec<u8>, String> {
    let mut child = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().map_err(|e| format!("REMOTE_RUNTIME_SPAWN_FAILED: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() { break; }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("REMOTE_RUNTIME_START_TIMEOUT: Windows process command did not return".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!("REMOTE_RUNTIME_SPAWN_FAILED: {}", String::from_utf8_lossy(&output.stderr)));
    }
    Ok(output.stdout)
}
