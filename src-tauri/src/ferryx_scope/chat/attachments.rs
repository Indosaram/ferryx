use super::{ChatError, Result};
use crate::scoped_contracts::{AttachmentReceipt, TargetRef};
use serde_json::Value;
/// Implement with the shared staging service. Resolve only after rechecking receipt
/// hash, signature, target host and private jail; paths never come from clients.
pub trait StagedAttachments {
 fn verified_input(&self, target: &TargetRef, receipt: &AttachmentReceipt) -> Result<Value>;
}
pub fn turn_input(_target:&TargetRef,text:&str,_receipts:&[AttachmentReceipt],_staging:&impl StagedAttachments)->Result<Vec<Value>> {
 if text.len()>65536 {return Err(ChatError::Limit);}
 Ok(vec![serde_json::json!({"type":"text","text":text})])
}
