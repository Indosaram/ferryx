extern crate ferryx_lib;
pub use ferryx_lib::scoped_contracts::*;
pub type Result<T> = std::result::Result<T,ScopeError>;
pub fn error(code: ScopeErrorCode,message:impl Into<String>)->ScopeError { ScopeError {code,message:message.into(),retryable:false,details:serde_json::Value::Null} }
use std::collections::HashMap;
#[path="lease.rs"] mod lease;
