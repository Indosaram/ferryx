//! Executable mhc JSONL peers shared by transport and chat integration tests.
use serde_json::{json, Value};

pub(crate) struct MockLog {
    pub frames: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
    pub session_file: std::path::PathBuf,
    changed: tokio::sync::watch::Receiver<u64>,
    dir: std::path::PathBuf,
}

impl Drop for MockLog {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl MockLog {
    pub async fn wait_for(&self, predicate: impl Fn(&[Value]) -> bool) {
        let mut changed = self.changed.clone();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if predicate(&self.frames.lock().unwrap()) {
                    return;
                }
                changed.changed().await.expect("peer log stays open");
            }
        })
        .await
        .expect("peer emitted the expected command");
    }
}

pub(crate) struct Script {
    pub messages: Vec<String>,
    pub approval: bool,
    pub live: bool,
}

/// Acknowledged logging is test instrumentation; RPC uses stdin/stdout exclusively.
pub(crate) async fn serve(script: Script) -> (String, MockLog) {
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = super::supervisor::agent_dir()
        .expect("test agent directory")
        .join("sessions")
        .join(format!(
            "ferryx-mhc-peer-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
    std::fs::create_dir_all(&dir).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let frames = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (changed, rx) = tokio::sync::watch::channel(0u64);
    let log = frames.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let log = log.clone();
            let changed = changed.clone();
            tokio::spawn(async move {
                let (reader, mut writer) = stream.into_split();
                let mut lines = BufReader::new(reader).lines();
                // A stopped turn kills the fixture's process group, which resets this socket.
                while let Ok(Some(line)) = lines.next_line().await {
                    log.lock()
                        .unwrap()
                        .push(serde_json::from_str(&line).unwrap());
                    changed.send_modify(|version| *version += 1);
                    if writer.write_all(b"ok\n").await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    let binary = dir.join("mhc-peer");
    let session_file = dir.join("session.jsonl");
    let session_id = uuid::Uuid::new_v4().to_string();
    std::fs::write(
        &session_file,
        format!("{}\n", json!({"type":"session", "id":session_id})),
    )
    .unwrap();
    let settings = json!({"port": port, "messages": script.messages,
        "approval": script.approval, "live": script.live, "sessionFile": session_file, "sessionId": session_id});
    let source = format!(
        "#!/usr/bin/env python3\nimport json, socket, sys\ncfg = json.loads({:?})\n",
        settings.to_string()
    ) + r#"
required = ['--mode', 'rpc', '--no-model-fallback']
assert sys.argv[1:1 + len(required)] == required, sys.argv
log = socket.create_connection(('127.0.0.1', cfg['port'])).makefile('rwb', buffering=0)
# Every turn is a fresh process: the model comes from this launch's --model flag.
launch_model = sys.argv[sys.argv.index('--model') + 1] if '--model' in sys.argv else None
model = launch_model.split('/', 1)[-1] if launch_model else None
log.write((json.dumps({'type':'fixture_launch', 'args':sys.argv[1:], 'model':launch_model}) + '\n').encode())
assert log.readline() == b'ok\n'
def emit(value):
    print(json.dumps(value), flush=True)
def finish():
    for index, text in enumerate(cfg['messages']):
        emit({'type':'message_end', 'message':{
            'id':f'assistant-{index}', 'timestamp':index,
            'role':'assistant', 'content':[{'type':'text','text':text}], 'stopReason':'stop'}})
    emit({'type':'agent_end', 'messages':[], 'willRetry':False})
for line in sys.stdin:
    frame = json.loads(line)
    log.write((json.dumps(frame) + '\n').encode())
    assert log.readline() == b'ok\n'
    kind = frame['type']
    if kind == 'extension_ui_response':
        finish()
        continue
    assert 'sessionId' not in frame
    if kind in ['set_model', 'set_thinking_level', 'set_auto_retry']:
        emit({'type':'response', 'id':frame.get('id'), 'command':kind, 'success':False,
              'error':'persists global settings'})
        continue
    response = {'type':'response', 'id':frame.get('id'), 'command':kind, 'success':True}
    if kind == 'get_state':
        response['data'] = {'sessionId':cfg['sessionId'], 'sessionFile':cfg['sessionFile']}
    emit(response)
    if kind == 'prompt' and cfg['messages'] == ['__provider503__']:
        emit({'type':'agent_start'})
        if model not in ['gpt-6-luna', 'gemini-4-argon-high']:
            emit({'type':'message_end','message':{'role':'assistant','stopReason':'error','errorMessage':'503 no available accounts'}})
        else:
            emit({'type':'message_end','message':{'id':'fallback','role':'assistant','content':[{'type':'text','text':'recovered'}]}})
            emit({'type':'agent_end','messages':[],'willRetry':False})
        continue
    if kind == 'prompt':
        emit({'type':'agent_start'})
    if kind == 'prompt' and not cfg['live']:
        if cfg['approval']:
            emit({'type':'extension_ui_request', 'id':'77',
                  'method':'confirm', 'title':'Run command', 'message':'git push origin main'})
        else:
            finish()
"#;
    std::fs::write(&binary, source).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    (
        binary.to_string_lossy().into_owned(),
        MockLog {
            frames,
            session_file,
            changed: rx,
            dir,
        },
    )
}
