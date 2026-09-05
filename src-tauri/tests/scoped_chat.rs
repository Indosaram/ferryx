pub use ferryx_lib::scoped_contracts;
#[path = "../src/ferryx_scope/chat/mod.rs"]
mod chat;
use chat::*;
use scoped_contracts::*;
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::process::Command;

struct Registry(Arc<Mutex<bool>>);
struct Guard(Arc<Mutex<bool>>);
impl Drop for Guard { fn drop(&mut self) { *self.0.lock().unwrap() = false; } }
impl Claim for Guard { fn bind(&self, _: ConversationClaimKey) -> Result<()> { Ok(()) } }
impl Claims for Registry {
 fn acquire(&self, _: ConversationOwner, _: Option<ConversationClaimKey>) -> Result<Box<dyn Claim>> {
  let mut owned = self.0.lock().unwrap(); if *owned { return Err(ChatError::ProviderOwned); } *owned = true; Ok(Box::new(Guard(self.0.clone())))
 }
}
fn target() -> TargetRef { TargetRef {host_id:"qa".into(),owner_id:"daemon".into(),epoch:Epoch(1),backend_session_id:"chat".into()} }
fn peer() -> Command {
 let mut cmd = Command::new("python3"); cmd.arg("-u").arg("-c").arg(include_str!("../src/ferryx_scope/chat/peer.py")); cmd
}
#[tokio::test]
async fn native_owned_thread_cannot_resume() {
 let result = Supervisor::spawn(peer(),target(),Arc::new(Registry(Arc::new(Mutex::new(true)))),Some("native".into())).await;
 assert!(matches!(result, Err(ChatError::ProviderOwned)));
}
struct Staging;
impl chat::attachments::StagedAttachments for Staging {
 fn verified_input(&self,_:&TargetRef,r:&AttachmentReceipt)->Result<serde_json::Value>{Ok(json!({"type":"localImage","path":format!("/private/{}",r.attachment_id)}))}
}
#[test]
fn staged_receipts_reject_foreign_host_and_include_verified_input(){
 let mut receipt=AttachmentReceipt{host_id:"foreign".into(),attachment_id:"opaque".into(),sha256:"a".repeat(64),size_bytes:4,media_type:AttachmentMediaType::Png};
 assert_eq!(chat::attachments::turn_input(&target(),"message",&[receipt.clone()],&Staging),Err(ChatError::Invalid));
 receipt.host_id="qa".into();
 let input=chat::attachments::turn_input(&target(),"message",&[receipt],&Staging).unwrap();
 assert_eq!(input[1]["type"],"localImage");
}
#[tokio::test]
async fn real_stdio_callbacks_two_client_race_stale_and_exit() {
 let owned=Arc::new(Mutex::new(false));
 let s=Supervisor::spawn(peer(),target(),Arc::new(Registry(owned.clone())),None).await;
 assert!(s.is_ok(), "real stdio supervisor must start: {:?}", s.err());
 let s=s.unwrap();
 assert_eq!(s.request("thread/start",json!({"ephemeral":true})).await.unwrap()["thread"]["id"],"thread-qa");
 s.request("turn/start",json!({"threadId":"thread-qa","input":[]})).await.unwrap();
 let event=s.next().await.unwrap(); assert_eq!(event["id"],json!("callback-real"));
 assert_eq!(s.callback(event["id"].clone(),"wrong","turn-qa",json!({"decision":"accept"})).await,Err(ChatError::Stale));
 let (a,b)=tokio::join!(s.callback(event["id"].clone(),"thread-qa","turn-qa",json!({"decision":"accept"})),s.callback(event["id"].clone(),"thread-qa","turn-qa",json!({"decision":"decline"})));
 assert_eq!(usize::from(a.is_ok())+usize::from(b.is_ok()),1);
 assert!(a==Err(ChatError::Stale)||b==Err(ChatError::Stale));
 let q=s.next().await.unwrap(); assert_eq!(q["id"],17);
 s.callback(json!(17),"thread-qa","turn-qa",json!({"answers":{"choice":{"answers":["A"]}}})).await.unwrap();
 let acknowledged=s.next().await.unwrap(); assert_eq!(acknowledged["params"]["answers"]["choice"]["answers"][0],"A");
 assert_eq!(s.next().await,Err(ChatError::Exited));
 s.stop().await.unwrap(); assert!(!*owned.lock().unwrap());
}
