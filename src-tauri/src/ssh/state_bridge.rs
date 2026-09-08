use super::{direct, runtime, SshHost};
use crate::ipc::{IpcError, IpcErrorCode};
use std::{process::Stdio, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdout};
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct StateEndpoint {
    pub port: u16,
    pub token: String,
    pub process_id: u32,
}

pub struct StateBridge {
    child: Child,
    reader: BufReader<ChildStdout>,
    pub endpoint: StateEndpoint,
}

impl StateBridge {
    pub async fn start(
        host: &SshHost,
        environment: &runtime::RemoteEnvironment,
    ) -> Result<Self, IpcError> {
        let token = uuid::Uuid::new_v4().simple().to_string();
        let lifetime = r#"using System.Diagnostics; using System.Threading.Tasks;
public static class FerryxSshLifetime {
    public static Task Observe(int id) {
        var done = new TaskCompletionSource<bool>();
        var process = Process.GetProcessById(id);
        process.EnableRaisingEvents = true;
        process.Exited += (s, e) => { done.TrySetResult(true); process.Dispose(); };
        if (process.HasExited) { done.TrySetResult(true); }
        return done.Task;
    }
}"#;
        let script = format!(
            r#"
Add-Type -TypeDefinition '{lifetime}'
$processes=@{{}}
foreach($p in (Get-CimInstance Win32_Process)) {{ $processes[[int]$p.ProcessId]=$p }}
$ancestor=$processes[[int]$PID]
while($ancestor -and $ancestor.Name -notlike 'sshd*') {{ $ancestor=$processes[[int]$ancestor.ParentProcessId] }}
if(!$ancestor) {{ throw 'SSH session owner is unavailable' }}
$ownerExit=[FerryxSshLifetime]::Observe($ancestor.ProcessId)
$listener=New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback,0)
$listener.Start()
try {{
    [Console]::WriteLine('FERRYX_STATE_V1_{token}:' + $listener.LocalEndpoint.Port + ':' + $PID)
    while (!$ownerExit.IsCompleted) {{
        $accept=$listener.AcceptTcpClientAsync()
        $done=[Threading.Tasks.Task]::WhenAny([Threading.Tasks.Task[]]@($accept,$ownerExit)).GetAwaiter().GetResult()
        if ($ownerExit.IsCompleted) {{ break }}
        $client=$accept.Result
        try {{
            $client.ReceiveTimeout=1000
            $s=$client.GetStream()
            $buffer=New-Object IO.MemoryStream
            $deadline=[Diagnostics.Stopwatch]::StartNew()
            try {{
                while ($buffer.Length -le 16384 -and $deadline.ElapsedMilliseconds -lt 1000 -and !$ownerExit.IsCompleted) {{
                    $b=$s.ReadByte()
                    if ($b -eq -1 -or $b -eq 10) {{ break }}
                    $buffer.WriteByte([byte]$b)
                }}
                if ($buffer.Length -le 16384) {{
                    $line=[Text.Encoding]::UTF8.GetString($buffer.ToArray())
                    $report=ConvertFrom-Json -InputObject $line
                    if ($report.token -ceq '{token}') {{ [Console]::WriteLine($line) }}
                }}
            }} finally {{ $buffer.Dispose() }}
        }} catch {{ [Console]::Error.WriteLine('Invalid agent state report') }}
        finally {{ $client.Close() }}
    }}
}} finally {{ $listener.Stop() }}
"#
        );
        let plan = direct::ssh_plan(host, format!(
            "{} -NoLogo -NoProfile -NonInteractive -Command -", environment.executor.program()
        ), false)?;
        let mut child = tokio::process::Command::new(&plan.program)
            .args(&plan.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| runtime::error(IpcErrorCode::IoError, "integration", &e.to_string()))?;
        let script = format!(
            "$ProgressPreference='SilentlyContinue'; $ErrorActionPreference='Stop'; try {{ {} }} catch {{ [Console]::Error.Write($_.Exception.Message); exit 1 }}\n",
            format!("Invoke-Expression {}", runtime::powershell_data(&script))
        );
        let mut stdin = child.stdin.take().ok_or_else(|| IpcError::internal("Missing SSH state input"))?;
        stdin.write_all(script.as_bytes()).await
            .map_err(|e| runtime::error(IpcErrorCode::IoError, "integration", &e.to_string()))?;
        drop(stdin);
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| IpcError::internal("Missing SSH state stream"))?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let prefix = format!("FERRYX_STATE_V1_{token}:");
        let (port, process_id) = tokio::time::timeout(Duration::from_secs(12), async {
            loop {
                line.clear();
                if (&mut reader).take(1025).read_line(&mut line).await? == 0 || line.len() > 1024 {
                    return Err(std::io::Error::other("Missing SSH state listener response"));
                }
                if let Some(fields) = line.trim().strip_prefix(&prefix) {
                    let (port, process_id) = fields.split_once(':')
                        .ok_or_else(|| std::io::Error::other("Malformed state listener response"))?;
                    return Ok((port.parse::<u16>().map_err(std::io::Error::other)?,
                        process_id.parse::<u32>().map_err(std::io::Error::other)?));
                }
            }
        })
        .await
        .map_err(|_| {
            runtime::error(
                IpcErrorCode::IoError,
                "integration",
                "SSH state listener timed out",
            )
        })?
        .map_err(|e| runtime::error(IpcErrorCode::IoError, "integration", &e.to_string()))?;
        if port == 0 || process_id == 0 {
            return Err(runtime::error(
                IpcErrorCode::ParseError,
                "integration",
                "Invalid state listener port",
            ));
        }
        Ok(Self {
            child,
            reader,
            endpoint: StateEndpoint { port, token, process_id },
        })
    }

    pub async fn next_report(&mut self, session_id: &str) -> Option<String> {
        loop {
            let mut line = String::new();
            let count = (&mut self.reader)
                .take(16385)
                .read_line(&mut line)
                .await
                .ok()?;
            if count == 0 || count > 16384 || !line.ends_with('\n') {
                return None;
            }
            if let Some(report) = authenticated_report(&line, session_id, &self.endpoint.token) {
                return Some(report);
            }
        }
    }

    pub async fn close(mut self) {
        let _ = self.child.kill().await;
    }

    pub async fn follow<F: FnMut(&str)>(
        mut self,
        session_id: String,
        mut lifecycle: broadcast::Receiver<Vec<u8>>,
        mut report: F,
    ) {
        let closed = async move {
            loop {
                if matches!(
                    lifecycle.recv().await,
                    Err(broadcast::error::RecvError::Closed)
                ) {
                    break;
                }
            }
        };
        tokio::pin!(closed);
        loop {
            tokio::select! {
                line = self.next_report(&session_id) => match line {
                    Some(line) => report(&line),
                    None => break,
                },
                _ = &mut closed => break,
            }
        }
        self.close().await;
    }
}

fn authenticated_report(line: &str, session_id: &str, token: &str) -> Option<String> {
    let mut value: serde_json::Value = serde_json::from_str(line).ok()?;
    let object = value.as_object_mut()?;
    if object.get("token")?.as_str()? != token || object.get("sessionId")?.as_str()? != session_id {
        return None;
    }
    object.remove("token");
    Some(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn state_reports_require_both_session_and_token() {
        let line = r#"{"sessionId":"one","token":"secret","state":"idle"}"#;
        assert!(authenticated_report(line, "two", "secret").is_none());
        assert!(authenticated_report(line, "one", "wrong").is_none());
        assert_eq!(
            authenticated_report(line, "one", "secret").unwrap(),
            r#"{"sessionId":"one","state":"idle"}"#
        );
    }
}
