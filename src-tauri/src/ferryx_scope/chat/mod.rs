use crate::scoped_contracts::*;
pub mod attachments;
use serde_json::{json, Value};
use std::{collections::HashMap, process::Stdio, sync::Arc, time::Duration};
use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, sync::{mpsc, oneshot, Mutex}, time::timeout};
use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatError { Unsupported, ProviderOwned, Stale, Invalid, Exited, Timeout, Protocol, Limit }
pub type Result<T> = std::result::Result<T, ChatError>;
/// The integrator implements this using the single native/managed registry.
/// A guard releases ONLY after the supervisor confirms child exit.
pub trait Claim: Send + Sync { fn bind(&self, key: ConversationClaimKey) -> Result<()>; }
pub trait Claims: Send + Sync { fn acquire(&self, owner: ConversationOwner, key: Option<ConversationClaimKey>) -> Result<Box<dyn Claim>>; }
const FRAME_LIMIT: usize = 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(30);
enum Action {
 Request(String, Value, oneshot::Sender<Result<Value>>),
 Callback(Value, String, String, Value, oneshot::Sender<Result<()>>),
 Stop(oneshot::Sender<Result<()>>),
}
pub struct Supervisor { tx: mpsc::Sender<Action>, events: Mutex<mpsc::Receiver<Value>> }
impl Supervisor {
 pub async fn spawn(mut command: Command, target: TargetRef, claims: Arc<dyn Claims>, resume: Option<String>) -> Result<Self> {
  let key = resume.as_ref().map(|id| ConversationClaimKey {host_id:target.host_id.clone(),provider:CanonicalProvider::Codex,conversation_id:id.clone()});
  let claim = claims.acquire(ConversationOwner::Managed {target:target.clone()},key)?;
  let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true).spawn().map_err(|_|ChatError::Exited)?;
  let mut input = child.stdin.take().ok_or(ChatError::Protocol)?;
  let output = child.stdout.take().ok_or(ChatError::Protocol)?;
  let (frames_tx,mut frames)=mpsc::channel(32);
  let reader=tokio::spawn(async move {
   let mut reader=BufReader::new(output); let mut bytes=Vec::new();
   loop {
    let chunk=match reader.fill_buf().await {Ok(v) if !v.is_empty()=>v, _=>break};
    let end=chunk.iter().position(|b|*b==b'\n').map(|n|n+1).unwrap_or(chunk.len());
    if bytes.len()+end>FRAME_LIMIT {break;}
    bytes.extend_from_slice(&chunk[..end]); reader.consume(end);
    if bytes.last()==Some(&b'\n') {
     let value=match serde_json::from_slice::<Value>(&bytes) {Ok(v)=>v,Err(_)=>break}; bytes.clear();
     if frames_tx.send(value).await.is_err() {break;}
    }
   }
  });
  let (tx,mut rx)=mpsc::channel(32); let (events_tx,events)=mpsc::channel(256);
  tokio::spawn(async move {
   let mut pending:HashMap<u64,(String,oneshot::Sender<Result<Value>>)>=HashMap::new();
   let mut callbacks:HashMap<String,Value>=HashMap::new(); let mut serial=0u64;
   let mut thread=resume; let mut stop_reply=None; let mut exited=false;
   loop {
    tokio::select! {
     status=child.wait()=> { exited=status.is_ok(); break; }
     frame=frames.recv()=> {
      let Some(frame)=frame else {break};
      if frame.get("method").is_none() {
       if let Some(id)=frame["id"].as_u64() {
        if let Some((method,reply))=pending.remove(&id) {
         let mut result=if frame.get("error").is_some() {Err(ChatError::Protocol)} else {Ok(frame["result"].clone())};
         if matches!(method.as_str(),"thread/start"|"thread/resume") {
          result=result.and_then(|v| {
           let id=v["thread"]["id"].as_str().ok_or(ChatError::Protocol)?;
           if thread.as_ref().is_some_and(|old|old!=id) {return Err(ChatError::Protocol);}
           claim.bind(ConversationClaimKey {host_id:target.host_id.clone(),provider:CanonicalProvider::Codex,conversation_id:id.to_owned()})?;
           thread=Some(id.to_owned()); Ok(v)
          });
         }
         let failed=result.is_err() && matches!(method.as_str(),"thread/start"|"thread/resume");
         let _=reply.send(result); if failed {break;}
        }
       }
      } else {
       if frame.get("id").is_some() {
        let method=frame["method"].as_str().unwrap_or("");
        if !matches!(method,"item/commandExecution/requestApproval"|"item/fileChange/requestApproval"|"item/tool/requestUserInput") {
         if write_frame(&mut input,&json!({"id":frame["id"],"error":{"code":-32601,"message":"Unsupported callback"}})).await.is_err(){break;} continue;
        }
        if callbacks.len()>=64 || !(frame["id"].is_string()||frame["id"].is_i64()) || frame["params"]["threadId"].as_str()!=thread.as_deref() {break;}
        let key=frame["id"].to_string(); if callbacks.contains_key(&key) {break;}
        callbacks.insert(key,frame.clone());
       }
       if events_tx.try_send(frame).is_err() {break;}
      }
     }
     action=rx.recv()=> {
      match action {
       None=>break,
       Some(Action::Stop(reply))=> {stop_reply=Some(reply);break;}
       Some(Action::Request(method,params,reply))=> {
        if pending.len()>=64 {let _=reply.send(Err(ChatError::Limit));continue;}
        if !matches!(method.as_str(),"initialize"|"initialized"|"thread/start"|"thread/resume"|"turn/start"|"turn/interrupt"|"thread/items/list") {let _=reply.send(Err(ChatError::Unsupported));continue;}
        if method.starts_with("turn/") || method=="thread/items/list" {
         if thread.is_none() || params["threadId"].as_str()!=thread.as_deref() {let _=reply.send(Err(ChatError::Stale));continue;}
        }
        if method=="thread/start" && thread.is_some() {let _=reply.send(Err(ChatError::ProviderOwned));continue;}
        if method=="thread/resume" && (thread.is_none()||params["threadId"].as_str()!=thread.as_deref()) {let _=reply.send(Err(ChatError::Stale));continue;}
        serial+=1;
        let frame=if method=="initialized" {json!({"method":method})} else {json!({"id":serial,"method":method,"params":params})};
        if let Err(e)=write_frame(&mut input,&frame).await {let _=reply.send(Err(e));break;}
        if method=="initialized" {let _=reply.send(Ok(Value::Null));} else {pending.insert(serial,(method,reply));}
       }
       Some(Action::Callback(id,thread_id,turn,result,reply))=> {
        let key=id.to_string();
        let response=match callbacks.get(&key) {
         Some(frame) if frame["params"]["threadId"]==thread_id && frame["params"]["turnId"]==turn => validate_callback(frame,&result),
         _=>Err(ChatError::Stale),
        };
        if let Err(e)=response {let _=reply.send(Err(e));continue;}
        callbacks.remove(&key);
        let sent=write_frame(&mut input,&json!({"id":id,"result":result})).await;
        let failed=sent.is_err(); let _=reply.send(sent); if failed {break;}
       }
      }
     }
    }
   }
   reader.abort();
   let cleanup=if exited {Ok(())} else {child.kill().await.map_err(|_|ChatError::Exited)};
   // kill() waits for reaping; retain the registry fence if exit cannot be confirmed.
   if cleanup.is_ok() {drop(claim);} else {std::mem::forget(claim);}
   for (_,(_,reply)) in pending {let _=reply.send(Err(ChatError::Exited));}
   if let Some(reply)=stop_reply {let _=reply.send(cleanup);}
  });
  let supervisor=Self {tx,events:Mutex::new(events)};
  supervisor.request("initialize",json!({"clientInfo":{"name":"ferryx","version":"1"},"capabilities":{"experimentalApi":true}})).await?;
  supervisor.request("initialized",Value::Null).await?;
  Ok(supervisor)
 }
 pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
  let (tx,rx)=oneshot::channel();
  self.tx.try_send(Action::Request(method.into(),params,tx)).map_err(|e|match e {mpsc::error::TrySendError::Closed(_)=>ChatError::Exited,mpsc::error::TrySendError::Full(_)=>ChatError::Limit})?;
  match timeout(DEADLINE,rx).await {Ok(v)=>v.unwrap_or(Err(ChatError::Exited)),Err(_)=>{self.stop().await?;Err(ChatError::Timeout)}}
 }
 pub async fn callback(&self, id: Value, thread: &str, turn: &str, result: Value) -> Result<()> {
  let (tx,rx)=oneshot::channel(); self.tx.try_send(Action::Callback(id,thread.into(),turn.into(),result,tx)).map_err(|_|ChatError::Exited)?;
  timeout(DEADLINE,rx).await.map_err(|_|ChatError::Timeout)?.unwrap_or(Err(ChatError::Exited))
 }
 pub async fn next(&self) -> Result<Value> {timeout(DEADLINE,self.events.lock().await.recv()).await.map_err(|_|ChatError::Timeout)?.ok_or(ChatError::Exited)}
 pub async fn stop(&self) -> Result<()> {
  let (tx,rx)=oneshot::channel(); if self.tx.send(Action::Stop(tx)).await.is_err() {return Ok(());}
  timeout(DEADLINE,rx).await.map_err(|_|ChatError::Timeout)?.unwrap_or(Ok(()))
 }
}
async fn write_frame(input:&mut tokio::process::ChildStdin,value:&Value)->Result<()> {
 let mut bytes=serde_json::to_vec(value).map_err(|_|ChatError::Invalid)?;
 if bytes.len()>=FRAME_LIMIT {return Err(ChatError::Limit);} bytes.push(b'\n');
 timeout(DEADLINE,input.write_all(&bytes)).await.map_err(|_|ChatError::Timeout)?.map_err(|_|ChatError::Exited)
}
fn validate_callback(frame:&Value,result:&Value)->Result<()> {
 if frame["method"]=="item/tool/requestUserInput" {
  let questions=frame["params"]["questions"].as_array().ok_or(ChatError::Protocol)?;
  let answers=result["answers"].as_object().ok_or(ChatError::Invalid)?;
  if answers.len()!=questions.len() {return Err(ChatError::Invalid);}
  for question in questions {
   let id=question["id"].as_str().ok_or(ChatError::Protocol)?;
   let values=answers.get(id).and_then(|a|a["answers"].as_array()).ok_or(ChatError::Invalid)?;
   if values.is_empty()||values.len()>16||values.iter().any(|v|v.as_str().is_none_or(|s|s.len()>8192)) {return Err(ChatError::Invalid);}
  }
 } else if !matches!(result["decision"].as_str(),Some("accept"|"decline"|"cancel")) {return Err(ChatError::Invalid);}
 Ok(())
}
