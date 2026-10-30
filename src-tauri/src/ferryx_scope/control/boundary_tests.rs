use super::*;
use std::sync::{Arc,atomic::Ordering};
use futures_util::future::BoxFuture;
struct NoLauncher;
impl local::TaskLauncher for NoLauncher {
    fn allocate(&self,_:Value)->BoxFuture<'_,Result<Agent>> { Box::pin(async {Err(error(ScopeErrorCode::Unsupported,"fixture uses registered PTY"))}) }
    fn start(&self,_:TargetRef,_:Value)->BoxFuture<'_,Result<Value>> { Box::pin(async {Err(error(ScopeErrorCode::Unsupported,"fixture already started"))}) }
    fn stop(&self,_:TargetRef)->BoxFuture<'_,Result<Value>> { Box::pin(async {Err(error(ScopeErrorCode::Unsupported,"fixture cleanup owns stop"))}) }
}
/// Runs on every platform: the router, its error mapping and the local adapter's refusal for a
/// session the terminal service does not own, with no PTY involved.
#[tokio::test]
async fn loopback_router_maps_missing_sessions_and_auth_on_every_platform() {
    let terminals = Arc::new(crate::terminal::TerminalService::default());
    let target=TargetRef {host_id:"qa".into(),owner_id:"owner".into(),epoch:Epoch(1),backend_session_id:"absent-session".into()};
    let auth=crate::remote::AuthManager::new();
    let (token,device)=auth.exchange_pairing_code(&auth.create_pairing_code(crate::remote::DevicePermission::Control),"fixture").unwrap();
    let service=service::Service::new(auth,16);
    service.allow_control.store(true,Ordering::SeqCst); service.grant_hosts(device.id,vec!["qa".into()]);
    service.register_backend("qa".into(),"owner".into(),Epoch(1),true,Arc::new(local::LocalBackend {sessions:terminals,launcher:Arc::new(NoLauncher)})).await;
    service.inventory.lock().await.insert(Agent {target:target.clone(),workspace_id:"fixture".into(),label:"fixture".into(),state:"idle".into(),revision:0,source:TransitionSource::Lifecycle}).unwrap();
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address=listener.local_addr().unwrap();
    let (shutdown,closed)=tokio::sync::oneshot::channel::<()>();
    let app=router::router(service.clone());
    let server=tokio::spawn(async move {axum::serve(listener,app).with_graceful_shutdown(async {let _=closed.await;}).await.unwrap();});
    let client=reqwest::Client::builder().no_proxy().build().unwrap();
    let base=format!("http://{address}/api/v1");
    let agent=format!("{base}/agents/{}",router::opaque_target(&target));
    assert_eq!(client.get(format!("{base}/agents")).send().await.unwrap().status(),401);
    for path in ["agents","hosts","events"] {
        assert_eq!(client.get(format!("{base}/{path}")).bearer_auth(&token).send().await.unwrap().status(),200,"{path}");
    }
    let body=serde_json::json!({"requestId":"prompt-absent","params":{"text":"hello\n"}}).to_string();
    let prompt=client.post(format!("{agent}/prompt")).bearer_auth(&token).header("content-type","application/json").body(body).send().await.unwrap();
    assert_eq!(prompt.status(),409,"a session the terminal service does not own is expired");
    assert_eq!(client.get(format!("{agent}/messages?limit=10")).bearer_auth(&token).send().await.unwrap().status(),409);
    assert_eq!(client.get(format!("{agent}/wait?timeoutMs=1")).bearer_auth(&token).send().await.unwrap().status(),504);
    assert_eq!(client.get(format!("{base}/agents/not-base64!/messages")).bearer_auth(&token).send().await.unwrap().status(),400);
    let create=serde_json::json!({"requestId":"create-1","params":{"hostId":"qa"}}).to_string();
    assert_eq!(client.post(format!("{base}/agents")).bearer_auth(&token).header("content-type","application/json").body(create).send().await.unwrap().status(),422);
    shutdown.send(()).unwrap(); server.await.unwrap();
}
#[cfg(unix)]
#[tokio::test]
async fn loopback_router_delivers_exactly_once_to_real_disposable_pty() {
    let root = tempfile::tempdir().unwrap();
    let terminals = Arc::new(crate::terminal::TerminalService::default());
    let mut cmd = portable_pty::CommandBuilder::new("/bin/cat");
    cmd.cwd(root.path()); cmd.env("HOME",root.path());
    let (id,mut output) = terminals.pty_manager().spawn(cmd,80,24).unwrap();
    terminals.output_hub().register_session(&id);
    let hub = terminals.output_hub().clone(); let pump_id=id.clone();
    let pump = tokio::spawn(async move {while let Some(bytes)=output.recv().await {hub.publish(&pump_id,bytes);} });
    let mut attachment=terminals.attach_with_sequence(&id,None).unwrap();
    let target=TargetRef {host_id:"qa".into(),owner_id:"owner".into(),epoch:Epoch(1),backend_session_id:id.clone()};
    let auth=crate::remote::AuthManager::new();
    let (token,device)=auth.exchange_pairing_code(&auth.create_pairing_code(crate::remote::DevicePermission::Control),"fixture").unwrap();
    let service=service::Service::new(auth,16);
    service.allow_control.store(true,Ordering::SeqCst); service.grant_hosts(device.id,vec!["qa".into()]);
    service.register_backend("qa".into(),"owner".into(),Epoch(1),true,Arc::new(local::LocalBackend {sessions:terminals.clone(),launcher:Arc::new(NoLauncher)})).await;
    service.inventory.lock().await.insert(Agent {target:target.clone(),workspace_id:"fixture".into(),label:"fixture".into(),state:"idle".into(),revision:0,source:TransitionSource::Lifecycle}).unwrap();
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address=listener.local_addr().unwrap();
    let (shutdown,closed)=tokio::sync::oneshot::channel::<()>();
    let app=router::router(service.clone());
    let server=tokio::spawn(async move {axum::serve(listener,app).with_graceful_shutdown(async {let _=closed.await;}).await.unwrap();});
    let client=reqwest::Client::new();
    let base=format!("http://{address}/api/v1/agents");
    assert_eq!(client.get(&base).send().await.unwrap().status(),401);
    let url=format!("{}/{}/prompt",base,router::opaque_target(&target));
    let body=serde_json::json!({"requestId":"prompt-once","target":target,"params":{"text":"CONTROL_QA_SENTINEL\n"}}).to_string();
    let response=client.post(&url).bearer_auth(&token).header("content-type","application/json").body(body.clone()).send().await.unwrap();
    assert_eq!(response.status(),200);
    let first=response.text().await.unwrap();
    let retry=client.post(&url).bearer_auth(&token).header("content-type","application/json").body(body).send().await.unwrap();
    assert_eq!(retry.text().await.unwrap(),first);
    tokio::time::timeout(std::time::Duration::from_secs(3),async {
        let mut bytes=Vec::new();
        loop {bytes.extend_from_slice(&attachment.receiver.recv().await.unwrap().bytes); if String::from_utf8_lossy(&bytes).contains("CONTROL_QA_SENTINEL") {break;} }
    }).await.unwrap();
    terminals.close_session(&id).await.unwrap();
    pump.abort(); let _=pump.await;
    shutdown.send(()).unwrap(); server.await.unwrap();
    let path=root.path().to_owned();root.close().unwrap();assert!(!path.exists());
    println!("CONTROL_BOUNDARY_CLEANUP: loopback closed, disposable PTY closed, pump joined, temporary root removed");
}
