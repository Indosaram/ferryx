use super::*;
use service::*;
use std::sync::{Arc, atomic::Ordering};
fn target() -> TargetRef { TargetRef { host_id: "a".into(), owner_id: "owner".into(), epoch: Epoch(1), backend_session_id: "registered".into() } }
struct Backend;
impl ControlBackend for Backend {
    fn create(&self, _: Value) -> futures_util::future::BoxFuture<'_, Result<Agent>> { Box::pin(async { Err(error(ScopeErrorCode::Unsupported, "allocation requires launcher")) }) }
    fn execute(&self, target: TargetRef, _: String, params: Value) -> futures_util::future::BoxFuture<'_, Result<Value>> { Box::pin(async move { Ok(serde_json::json!({"target":target,"accepted":params})) }) }
    fn read(&self, _: TargetRef, _: usize) -> futures_util::future::BoxFuture<'_, Result<Value>> { Box::pin(async { Ok(serde_json::json!([])) }) }
}
#[tokio::test]
async fn roles_dedupe_wait_and_revoke() {
    let auth = crate::remote::AuthManager::new();
    let (token, d) = auth.exchange_pairing_code(&auth.create_pairing_code(crate::remote::DevicePermission::Control), "qa").unwrap();
    let (view, vd) = auth.exchange_pairing_code(&auth.create_pairing_code(crate::remote::DevicePermission::View), "view").unwrap();
    let s = Service::new(auth, 8);
    s.register_backend("a".into(), "owner".into(), Epoch(1), true, Arc::new(Backend)).await;
    s.inventory.lock().await.insert(Agent {target:target(), workspace_id:"w".into(),label:"task".into(),state:"idle".into(),revision:0,source:TransitionSource::Lifecycle}).unwrap();
    s.grant_hosts(d.id.clone(), vec!["a".into()]); s.grant_hosts(vd.id, vec!["a".into()]);
    s.allow_control.store(true, Ordering::SeqCst);
    let e = MutationEnvelope {request_id:"once".into(),target:Some(target()),params:serde_json::json!({"text":"hello"})};
    assert_eq!(s.mutate(&view,"prompt",e.clone()).await.unwrap_err().code, ScopeErrorCode::Forbidden);
    let receipt = s.mutate(&token,"prompt",e.clone()).await.unwrap();
    assert_eq!(s.mutate(&token,"prompt",e.clone()).await.unwrap(), receipt);
    let mut conflict = e.clone(); conflict.params = serde_json::json!({"text":"different"});
    assert_eq!(s.mutate(&token,"prompt",conflict).await.unwrap_err().code, ScopeErrorCode::RequestConflict);
    let cursor = s.inventory.lock().await.snapshot().revision;
    let wait = s.wait(&token, target(), cursor, "turn.completed", 1000);
    tokio::pin!(wait);
    assert!(futures_util::poll!(&mut wait).is_pending());
    s.inventory.lock().await.report(&target(),TransitionKind::TaskComplete,TransitionSource::Provider {provider:CanonicalProvider::Codex,request_id:Some("turn".into())}).unwrap();
    assert_eq!(wait.await.unwrap().event_type,"turn.completed");
    let waiting = s.wait(&token,target(),s.inventory.lock().await.snapshot().revision,"turn.completed",1000);
    tokio::pin!(waiting);
    assert!(futures_util::poll!(&mut waiting).is_pending());
    s.revoke(&d.id);
    assert_eq!(waiting.await.unwrap_err().code,ScopeErrorCode::Unauthorized);
}
