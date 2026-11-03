//! Replays audited Slack rows through the production commitment ledger
//! (docs/design/hub-owner-reporting.md section 11, "Commitment DB replay").
//!
//! Usage: `commitment_replay <slack-audit-182.jsonl> <promises-20.json>`.
//!
//! The rows are walked in time order against an in-memory hub database. Before each row,
//! `commitments::sweep` runs at that row's KST time. Every hub (wawabot) row then goes through
//! `commitments::track`, the same function the Slack turn calls after a reply, with a thread ref
//! of `channel:thread_ts` (or `channel:ts` for a top-level message). A final sweep runs 24h after
//! the last row. The judge is scripted from the audit: `promise` for a labelled promise row, and
//! `delivers` when the row being tracked is the one the audit pairs with the candidate promise.
//!
//! Promises sent by another bot (Instinct) are not tracked: the hub only sees and tracks its own
//! replies, so those rows are listed under `not_tracked` instead of being faked.
//!
//! Output is one JSON object on stdout. Exit code 1 when a check fails.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, SecondsFormat, TimeZone, Utc};
use ferryx_hub::commitments;
use ferryx_hub::judge::Judge;
use serde_json::{json, Value};
use sqlx::Row;

/// The hub's own Slack bot in the audit.
const HUB_SENDER: &str = "wawabot";

struct AuditRow {
    i: u32,
    sender: String,
    channel: String,
    thread_ref: String,
    message_ref: String,
    at: DateTime<Utc>,
    text: String,
}

/// `2026. 10. 30. 오전 11:46:53` (KST) as UTC.
fn parse_kst(kst: &str) -> Option<DateTime<Utc>> {
    let parts: Vec<&str> = kst.split_whitespace().collect();
    let [year, month, day, half, clock] = parts.as_slice() else {
        return None;
    };
    let number = |part: &str| part.trim_end_matches('.').parse::<u32>().ok();
    let date = NaiveDate::from_ymd_opt(
        i32::try_from(number(year)?).ok()?,
        number(month)?,
        number(day)?,
    )?;
    let mut clock = clock.split(':').map(|part| part.parse::<u32>().ok());
    let (hour, minute, second) = (clock.next()??, clock.next()??, clock.next()??);
    let hour = match *half {
        "오전" => hour % 12,
        "오후" => hour % 12 + 12,
        _ => return None,
    };
    let local = date.and_hms_opt(hour, minute, second)?;
    let kst = FixedOffset::east_opt(9 * 3600)?;
    Some(kst.from_local_datetime(&local).single()?.with_timezone(&Utc))
}

fn stamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn fail(message: String) -> ! {
    eprintln!("{message}");
    std::process::exit(2);
}

fn load_rows(path: &str) -> Vec<AuditRow> {
    let raw = std::fs::read_to_string(path).unwrap_or_else(|error| fail(format!("{path}: {error}")));
    let mut rows = Vec::new();
    for (number, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line)
            .unwrap_or_else(|error| fail(format!("{path}:{}: {error}", number + 1)));
        let field = |name: &str| value.get(name).and_then(Value::as_str).unwrap_or_default().to_owned();
        let i = value
            .get("i")
            .and_then(Value::as_u64)
            .and_then(|i| u32::try_from(i).ok())
            .unwrap_or_else(|| fail(format!("{path}:{}: no row number", number + 1)));
        let kst = field("kst");
        let at = parse_kst(&kst).unwrap_or_else(|| fail(format!("row {i}: bad kst {kst:?}")));
        let channel = field("channel");
        let ts = field("ts");
        let thread = value
            .get("thread_ts")
            .and_then(Value::as_str)
            .map_or_else(|| ts.clone(), str::to_owned);
        rows.push(AuditRow {
            i,
            sender: field("sender"),
            thread_ref: format!("{channel}:{thread}"),
            message_ref: format!("{channel}:{ts}"),
            channel,
            at,
            text: field("text"),
        });
    }
    // Stable: rows sent in the same second keep their audited order.
    rows.sort_by_key(|row| (row.at, row.i));
    rows
}

/// `promise row -> fulfilling row` (`None` = never fulfilled).
fn load_pairs(path: &str) -> BTreeMap<u32, Option<u32>> {
    let raw = std::fs::read_to_string(path).unwrap_or_else(|error| fail(format!("{path}: {error}")));
    let value: Value = serde_json::from_str(&raw).unwrap_or_else(|error| fail(format!("{path}: {error}")));
    let row = |value: Option<&Value>| value.and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
    value
        .as_array()
        .unwrap_or_else(|| fail(format!("{path}: expected an array")))
        .iter()
        .map(|entry| {
            let promise = row(entry.get("i")).unwrap_or_else(|| fail(format!("{path}: entry without i")));
            (promise, row(entry.get("fulfilled_by")))
        })
        .collect()
}

/// The audit as a judge: `promise` for a labelled promise row, `delivers` when the current row
/// fulfils a promise whose text contains the candidate's `what`.
fn audit_judge(
    current: Arc<Mutex<u32>>,
    pairs: &BTreeMap<u32, Option<u32>>,
    texts: &HashMap<u32, String>,
) -> Judge {
    let promises: BTreeSet<u32> = pairs.keys().copied().collect();
    let mut delivered_by: HashMap<u32, Vec<String>> = HashMap::new();
    for (promise, fulfilled_by) in pairs {
        if let (Some(by), Some(text)) = (fulfilled_by, texts.get(promise)) {
            delivered_by.entry(*by).or_default().push(text.clone());
        }
    }
    Judge::scripted(move |_, state, options| {
        let row = *current.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if options.contains(&"promise") {
            return if promises.contains(&row) { "promise" } else { "none" }.to_owned();
        }
        let what = state
            .strip_prefix("promise:\n")
            .and_then(|rest| rest.split_once("\n\nnewer message:"))
            .map(|(what, _)| what.trim())
            .unwrap_or_default();
        let delivers = !what.is_empty()
            && delivered_by
                .get(&row)
                .is_some_and(|texts| texts.iter().any(|text| text.contains(what)));
        (if delivers { "delivers" } else { "not_yet" }).to_owned()
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, rows_path, pairs_path] = args.as_slice() else {
        fail("usage: commitment_replay <slack-audit-182.jsonl> <promises-20.json>".to_owned());
    };
    let rows = load_rows(rows_path);
    let pairs = load_pairs(pairs_path);
    let Some(last) = rows.last().map(|row| row.at) else {
        fail(format!("{rows_path}: no rows"));
    };
    let texts: HashMap<u32, String> = rows.iter().map(|row| (row.i, row.text.clone())).collect();
    let row_at: HashMap<u32, DateTime<Utc>> = rows.iter().map(|row| (row.i, row.at)).collect();
    let row_of_ref: HashMap<String, u32> =
        rows.iter().map(|row| (row.message_ref.clone(), row.i)).collect();
    let sender_of: HashMap<u32, String> = rows.iter().map(|row| (row.i, row.sender.clone())).collect();

    let pool = ferryx_hub::storage::memory()
        .await
        .unwrap_or_else(|error| fail(format!("memory database: {error}")));
    let current = Arc::new(Mutex::new(0_u32));
    let judge = audit_judge(current.clone(), &pairs, &texts);

    let mut follow_ups: Vec<(commitments::FollowUp, DateTime<Utc>)> = Vec::new();
    let mut tracked_rows = 0_usize;
    for row in &rows {
        match commitments::sweep(&pool, row.at).await {
            Ok(due) => follow_ups.extend(due.into_iter().map(|follow_up| (follow_up, row.at))),
            Err(error) => fail(format!("sweep before row {}: {error}", row.i)),
        }
        if row.sender != HUB_SENDER {
            continue;
        }
        *current.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = row.i;
        tracked_rows += 1;
        if let Err(error) = commitments::track(
            &pool,
            &judge,
            &row.channel,
            &row.thread_ref,
            &row.message_ref,
            &row.text,
            row.at,
        )
        .await
        {
            fail(format!("track row {}: {error}", row.i));
        }
    }
    let final_sweep = last + Duration::hours(24);
    match commitments::sweep(&pool, final_sweep).await {
        Ok(due) => follow_ups.extend(due.into_iter().map(|follow_up| (follow_up, final_sweep))),
        Err(error) => fail(format!("final sweep: {error}")),
    }
    let resweep = commitments::sweep(&pool, final_sweep + Duration::hours(24))
        .await
        .unwrap_or_else(|error| fail(format!("second final sweep: {error}")));

    let ledger = sqlx::query(
        "SELECT id, thread_ref, message_ref, what, due_at, state, fulfilled_by, followed_up_at
         FROM commitments ORDER BY created_at, rowid",
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_else(|error| fail(format!("read commitments: {error}")));

    let mut row_of_commitment: HashMap<String, u32> = HashMap::new();
    let mut state_of_row: HashMap<u32, (String, Option<u32>)> = HashMap::new();
    let mut commitment_rows = Vec::new();
    for entry in &ledger {
        let id: String = entry.get("id");
        let message_ref: String = entry.get("message_ref");
        let fulfilled_by: Option<String> = entry.get("fulfilled_by");
        let row = row_of_ref.get(&message_ref).copied();
        let fulfilled_row = fulfilled_by.as_ref().and_then(|by| row_of_ref.get(by).copied());
        let state: String = entry.get("state");
        if let Some(row) = row {
            row_of_commitment.insert(id.clone(), row);
            state_of_row.insert(row, (state.clone(), fulfilled_row));
        }
        commitment_rows.push(json!({
            "id": id,
            "row": row,
            "thread_ref": entry.get::<String, _>("thread_ref"),
            "message_ref": message_ref,
            "what": entry.get::<String, _>("what"),
            "due_at": entry.get::<String, _>("due_at"),
            "state": state,
            "fulfilled_by": fulfilled_by,
            "fulfilled_by_row": fulfilled_row,
            "followed_up_at": entry.get::<Option<String>, _>("followed_up_at"),
        }));
    }

    let mut follow_up_count: BTreeMap<u32, usize> = BTreeMap::new();
    let mut follow_up_rows = Vec::new();
    let mut after_fulfilment = Vec::new();
    let mut late_fulfilled = Vec::new();
    for (follow_up, at) in &follow_ups {
        let row = row_of_commitment.get(&follow_up.commitment_id).copied();
        if let Some(row) = row {
            *follow_up_count.entry(row).or_default() += 1;
            if let Some(Some(by)) = pairs.get(&row) {
                let fulfilled_at = row_at.get(by).copied();
                let entry = json!({
                    "row": row,
                    "followed_up_at": stamp(*at),
                    "fulfilled_by_row": by,
                    "fulfilled_at": fulfilled_at.map(stamp),
                });
                if fulfilled_at.is_some_and(|fulfilled_at| fulfilled_at <= *at) {
                    after_fulfilment.push(entry);
                } else {
                    late_fulfilled.push(entry);
                }
            }
        }
        follow_up_rows.push(json!({
            "commitment_id": follow_up.commitment_id,
            "row": row,
            "thread_ref": follow_up.thread_ref,
            "at": stamp(*at),
            "body_ko": follow_up.body_ko,
        }));
    }

    let mut not_tracked = Vec::new();
    let mut unfulfilled = Vec::new();
    let mut fulfilment_mismatches = Vec::new();
    for (promise, fulfilled_by) in &pairs {
        let sender = sender_of.get(promise).cloned().unwrap_or_default();
        if sender != HUB_SENDER {
            not_tracked.push(json!({
                "row": promise,
                "sender": sender,
                "fulfilled_by": fulfilled_by,
                "reason": "another bot's promise: the hub only tracks its own replies and is not a member of that conversation, so it never sees the message",
            }));
            continue;
        }
        let count = follow_up_count.get(promise).copied().unwrap_or(0);
        let state = state_of_row.get(promise).cloned();
        match fulfilled_by {
            None => unfulfilled.push(json!({
                "row": promise,
                "follow_ups": count,
                "state": state.as_ref().map(|(state, _)| state.clone()),
                "ok": count == 1,
            })),
            Some(by) => {
                let closed_right = matches!(&state, Some((state, Some(row))) if state == "fulfilled" && row == by);
                if !closed_right {
                    fulfilment_mismatches.push(json!({
                        "row": promise,
                        "expected_fulfilled_by": by,
                        "state": state.as_ref().map(|(state, _)| state.clone()),
                        "fulfilled_by_row": state.as_ref().and_then(|(_, row)| *row),
                    }));
                }
            }
        }
    }
    let unlabelled_follow_ups: Vec<u32> = follow_up_count
        .keys()
        .copied()
        .filter(|row| !pairs.contains_key(row))
        .collect();

    let pass = unfulfilled.iter().all(|entry| entry["ok"] == json!(true))
        && after_fulfilment.is_empty()
        && fulfilment_mismatches.is_empty()
        && unlabelled_follow_ups.is_empty()
        && resweep.is_empty();
    let report = json!({
        "inputs": {"rows": rows_path, "pairs": pairs_path},
        "rows": rows.len(),
        "hub_rows_tracked": tracked_rows,
        "final_sweep_at": stamp(final_sweep),
        "commitments": commitment_rows,
        "follow_ups": follow_up_rows,
        "checks": {
            "unfulfilled_hub_promises": unfulfilled,
            "followed_up_after_fulfilment": after_fulfilment,
            "fulfilled_after_due_and_followed_up_first": late_fulfilled,
            "fulfilment_mismatches": fulfilment_mismatches,
            "follow_ups_for_unlabelled_rows": unlabelled_follow_ups,
            "second_final_sweep_follow_ups": resweep.len(),
            "pass": pass,
        },
        "not_tracked": not_tracked,
    });
    match serde_json::to_string_pretty(&report) {
        Ok(text) => println!("{text}"),
        Err(error) => fail(format!("render report: {error}")),
    }
    if !pass {
        std::process::exit(1);
    }
}
