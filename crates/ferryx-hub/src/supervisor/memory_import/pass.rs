//! Retry ownership: one import item stays current until admitted or the pass stops.
use super::*;
use serde_json::Value;

pub(super) struct ImportItem<'a> {
    pub principal: &'a str,
    pub source: &'a str,
    pub project: Option<&'a str>,
    pub text: &'a str,
}

pub(super) async fn ingest_item(
    pool: &SqlitePool,
    memory: &dyn OwnerMemory,
    item: ImportItem<'_>,
) -> (Value, ImportReport) {
    let mut report = ImportReport::default();
    loop {
        let outcome = ingest(pool, memory, item.principal, ChannelProvider::Import,
            item.source, item.project, item.text).await;
        if let Some("ingested" | "skipped") = outcome["state"].as_str() {
            return (outcome, report);
        }
        let code = outcome["code"].as_str().unwrap_or("MEMORY_REFUSED");
        let detail = outcome["error"].as_str().unwrap_or(code);
        let compaction = detail.contains("while compaction is in progress");
        if compaction {
            report.refusals += 1;
        } else {
            report.failed += 1;
        }
        let terminal = match code {
            "MHC_ENGINE_EXITED" | "MHC_NOT_RUNNING" | "MHC_SESSION_CLOSED" | "MHC_BUDGET" => true,
            "MHC_TIMEOUT" if detail.contains("owner memory compaction") => {
                report.wait_timeouts += 1;
                true
            }
            _ => false,
        };
        if terminal || report.failed + report.refusals >= 3 {
            report.stop_reason = Some(if terminal { detail.to_owned() }
                else { format!("three consecutive refusals or failures: {detail}") });
            return (outcome, report);
        }
    }
}
