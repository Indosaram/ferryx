use super::*;
/// Inject the owner's monotonic millisecond clock. No wall-clock sleeps in tests.
#[derive(Default)]
pub struct Leases { active: HashMap<TargetRef, ControlLease> }
impl Leases {
    pub fn acquire(&mut self,target:TargetRef,device_id:String,lease_id:String,now:u64,ttl:u64)->Result<ControlLease> {
        let lease=ControlLease {target:target.clone(),device_id,lease_id,expires_at_ms:now+ttl};
        self.active.insert(target,lease.clone());Ok(lease)
    }
    pub fn revoke(&mut self,device:&str) { self.active.retain(|_,lease|lease.device_id!=device); }
}
#[test]
fn rejects_competing_controller_until_expiry_or_revoke() {
    let t=TargetRef {host_id:"h".into(),owner_id:"o".into(),epoch:Epoch(1),backend_session_id:"s".into()};
    let mut l=Leases::default();
    l.acquire(t.clone(),"one".into(),"lease-one".into(),10,100).unwrap();
    assert_eq!(l.acquire(t.clone(),"two".into(),"lease-two".into(),20,100).unwrap_err().code,ScopeErrorCode::ControlConflict);
    l.revoke("one");
    assert!(l.acquire(t,"two".into(),"lease-two".into(),20,100).is_ok());
}
