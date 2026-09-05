use super::{service::Service, *};
use axum::{extract::{Path, Query, State}, http::{HeaderMap, StatusCode}, response::{IntoResponse, Response}, routing::{get, post}, Json, Router};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use std::sync::Arc;
pub fn opaque_target(t: &TargetRef) -> String { URL_SAFE_NO_PAD.encode(serde_json::to_vec(t).expect("TargetRef serializes")) }
fn decode(t: &str) -> Result<TargetRef> {
    let bytes = URL_SAFE_NO_PAD.decode(t).map_err(|_| error(ScopeErrorCode::InvalidRequest,"invalid target encoding"))?;
    serde_json::from_slice(&bytes).map_err(|_| error(ScopeErrorCode::InvalidRequest,"invalid target identity"))
}
fn token(h: &HeaderMap) -> &str { h.get("authorization").and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).unwrap_or("") }
fn respond<T: Serialize>(id: String, r: Result<T>) -> Response {
    match r {
        Ok(data) => Json(ScopeResult::<T>::Success {ok: WireBool, data, request_id:id}).into_response(),
        Err(e) => {
            let status = match e.code {
                ScopeErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
                ScopeErrorCode::Forbidden => StatusCode::FORBIDDEN,
                ScopeErrorCode::NotFound => StatusCode::NOT_FOUND,
                ScopeErrorCode::TargetExpired | ScopeErrorCode::RequestConflict | ScopeErrorCode::ControlConflict | ScopeErrorCode::ProviderOwned => StatusCode::CONFLICT,
                ScopeErrorCode::Unsupported => StatusCode::UNPROCESSABLE_ENTITY,
                ScopeErrorCode::Timeout => StatusCode::GATEWAY_TIMEOUT,
                ScopeErrorCode::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
                ScopeErrorCode::InventoryIncomplete => StatusCode::CONFLICT,
                _ => StatusCode::BAD_REQUEST,
            };
            (status,Json(ScopeResult::<Value>::Failure {ok:WireBool,error:e,request_id:id})).into_response()
        }
    }
}
/// Merge into an authenticated gateway. This function opens no listener.
pub fn router(service: Arc<Service>) -> Router {
    Router::new().route("/api/v1/agents",get(list).post(create))
        .route("/api/v1/agents/{target}/{operation}",post(mutate))
        .route("/api/v1/agents/{target}/messages",get(read))
        .route("/api/v1/agents/{target}/wait",get(wait))
        .route("/api/v1/hosts",get(hosts))
        .route("/api/v1/events",get(events))
        .with_state(service)
}
async fn list(State(s): State<Arc<Service>>, h: HeaderMap) -> Response { respond(String::new(),s.list(token(&h)).await) }
async fn create(State(s): State<Arc<Service>>, h: HeaderMap, body: std::result::Result<Json<MutationEnvelope<Value>>, axum::extract::rejection::JsonRejection>) -> Response {
    match body { Ok(Json(e)) => respond(e.request_id.clone(),s.mutate(token(&h),"create",e).await), Err(_) => respond::<Value>(String::new(),Err(error(ScopeErrorCode::InvalidRequest,"invalid mutation envelope"))) }
}
async fn mutate(State(s): State<Arc<Service>>, Path((t,op)): Path<(String,String)>, h: HeaderMap, body: std::result::Result<Json<MutationEnvelope<Value>>, axum::extract::rejection::JsonRejection>) -> Response {
    let result = async {
        let Json(mut e) = body.map_err(|_| error(ScopeErrorCode::InvalidRequest,"invalid mutation envelope"))?;
        let target = decode(&t)?;
        if e.target.as_ref().is_some_and(|v| v != &target) { return Err(error(ScopeErrorCode::InvalidRequest,"path and envelope target differ")); }
        e.target = Some(target);
        Ok(e)
    }.await;
    match result { Ok(e) => respond(e.request_id.clone(),s.mutate(token(&h),&op,e).await), Err(e) => respond::<Value>(String::new(),Err(e)) }
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct Params { limit: Option<usize>, after_sequence: Option<u64>, until: Option<String>, timeout_ms: Option<u64> }
async fn read(State(s):State<Arc<Service>>,Path(t):Path<String>,Query(p):Query<Params>,h:HeaderMap)->Response {
    respond(String::new(),async {s.read(token(&h),decode(&t)?,p.limit.unwrap_or(100)).await}.await)
}
async fn wait(State(s):State<Arc<Service>>,Path(t):Path<String>,Query(p):Query<Params>,h:HeaderMap)->Response {
    respond(String::new(),async {s.wait(token(&h),decode(&t)?,p.after_sequence.unwrap_or(0),p.until.as_deref().unwrap_or("turn.completed"),p.timeout_ms.unwrap_or(30000)).await}.await)
}
async fn hosts(State(s):State<Arc<Service>>,h:HeaderMap)->Response {
    let result = async {
        s.authorize(token(&h),None,false)?;
        let i = s.inventory.lock().await;
        Ok(i.hosts.iter().filter(|(host,_)|s.authorize(token(&h),Some(host),false).is_ok()).map(|(host,(owner,epoch,complete))|serde_json::json!({"hostId":host,"ownerId":owner,"epoch":epoch,"complete":complete})).collect::<Vec<_>>())
    }.await;
    respond(String::new(),result)
}
async fn events(State(s):State<Arc<Service>>,Query(p):Query<Params>,h:HeaderMap)->Response {
    let result = async {
        s.authorize(token(&h),None,false)?;
        let replay = s.inventory.lock().await.replay(p.after_sequence.unwrap_or(0));
        Ok(match replay {
            EventReplay::Events {mut events,after_sequence} => {events.retain(|e|s.authorize(token(&h),Some(&e.target.host_id),false).is_ok()); EventReplay::Events {events,after_sequence}},
            EventReplay::Gap {after_sequence,..} => EventReplay::Gap {snapshot:s.list(token(&h)).await?,after_sequence},
        })
    }.await;
    respond(String::new(),result)
}
