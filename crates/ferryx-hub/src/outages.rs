//! Known outages, reported once and again only on a state change (docs/design/hub-owner-reporting.md section 5).
//!
//! A failing job or login is an `outages` row keyed by `(subject, cause)`. The first failure
//! yields one notice; repeated failures only move `last_seen`; a recovery yields one recovery
//! notice; a new cause is a new row and so a new notice.
use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::error::Result;

/// The outage state a notice reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutageState {
    Active,
    Recovered,
}

impl OutageState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Recovered => "recovered",
        }
    }
}

/// One owner-facing outage message, in the owner's language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub body: String,
    pub state: OutageState,
}

fn stamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn active_body(subject: &str, cause: &str) -> String {
    crate::locale::text(
        &format!("{subject}이(가) 실패하고 있습니다. 원인: {cause}. 해결하려면 이 부분을 처리해 주세요. 복구되면 한 번 더 알려드리겠습니다."),
        &format!("{subject} is failing. Cause: {cause}. Please take care of it; I will tell you once more when it recovers."),
    )
}

fn recovered_body(subject: &str, cause: &str) -> String {
    crate::locale::text(
        &format!("{subject}이(가) 복구되었습니다. (이전 원인: {cause})"),
        &format!("{subject} has recovered. (Previous cause: {cause})"),
    )
}

/// Records one observation of `subject` and returns the notice to send, if the state changed.
///
/// `failing` with `cause` opens or refreshes the `(subject, cause)` row. A success
/// (`failing == false`) recovers every active row of `subject`; `cause` is then ignored.
pub async fn observe(
    pool: &SqlitePool,
    subject: &str,
    cause: &str,
    failing: bool,
    now: DateTime<Utc>,
) -> Result<Option<Notice>> {
    let at = stamp(now);
    if !failing {
        return recover(pool, subject, &at).await;
    }
    let existing = sqlx::query("SELECT id, state FROM outages WHERE subject = ? AND cause = ?")
        .bind(subject)
        .bind(cause)
        .fetch_optional(pool)
        .await?;
    match existing {
        Some(row) if row.get::<String, _>("state") == "active" => {
            sqlx::query("UPDATE outages SET last_seen = ? WHERE id = ?")
                .bind(&at)
                .bind(row.get::<String, _>("id"))
                .execute(pool)
                .await?;
            Ok(None)
        }
        Some(row) => {
            let reopened = sqlx::query(
                "UPDATE outages SET state = 'active', first_seen = ?, last_seen = ?, reported_at = ?,
                 last_reported_state = 'active' WHERE id = ? AND state = 'recovered'",
            )
            .bind(&at)
            .bind(&at)
            .bind(&at)
            .bind(row.get::<String, _>("id"))
            .execute(pool)
            .await?;
            Ok((reopened.rows_affected() == 1).then(|| Notice {
                body: active_body(subject, cause),
                state: OutageState::Active,
            }))
        }
        None => {
            let inserted = sqlx::query(
                "INSERT INTO outages(id, subject, cause, state, first_seen, last_seen, reported_at, last_reported_state)
                 VALUES(?,?,?,'active',?,?,?,'active')
                 ON CONFLICT(subject, cause) DO NOTHING",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(subject)
            .bind(cause)
            .bind(&at)
            .bind(&at)
            .bind(&at)
            .execute(pool)
            .await?;
            if inserted.rows_affected() == 0 {
                // A concurrent writer inserted first and reported the notice.
                sqlx::query("UPDATE outages SET last_seen = ? WHERE subject = ? AND cause = ?")
                    .bind(&at)
                    .bind(subject)
                    .bind(cause)
                    .execute(pool)
                    .await?;
                return Ok(None);
            }
            Ok(Some(Notice {
                body: active_body(subject, cause),
                state: OutageState::Active,
            }))
        }
    }
}

async fn recover(pool: &SqlitePool, subject: &str, at: &str) -> Result<Option<Notice>> {
    let causes: Vec<String> = sqlx::query_scalar(
        "SELECT cause FROM outages WHERE subject = ? AND state = 'active' ORDER BY first_seen",
    )
    .bind(subject)
    .fetch_all(pool)
    .await?;
    if causes.is_empty() {
        return Ok(None);
    }
    let recovered = sqlx::query(
        "UPDATE outages SET state = 'recovered', last_seen = ?, reported_at = ?,
         last_reported_state = 'recovered' WHERE subject = ? AND state = 'active'",
    )
    .bind(at)
    .bind(at)
    .bind(subject)
    .execute(pool)
    .await?;
    if recovered.rows_affected() == 0 {
        return Ok(None);
    }
    Ok(Some(Notice {
        body: recovered_body(subject, &causes.join(", ")),
        state: OutageState::Recovered,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .expect("test timestamp")
            .with_timezone(&Utc)
    }

    #[tokio::test]
    async fn repeated_failure_reports_once_and_again_on_recovery() {
        let pool = crate::storage::memory().await.expect("memory pool");
        let subject = "Freelancer";
        let cause = "로그아웃됨 (다시 로그인 필요)";

        let first = observe(&pool, subject, cause, true, at("2026-10-28T00:00:00Z"))
            .await
            .expect("first failure")
            .expect("first failure is reported");
        assert_eq!(first.state, OutageState::Active);
        assert!(first.body.contains("Freelancer"));
        assert!(first.body.contains("로그인"));

        for hour in 1..5 {
            let repeated = observe(
                &pool,
                subject,
                cause,
                true,
                at(&format!("2026-10-28T0{hour}:00:00Z")),
            )
            .await
            .expect("repeated failure");
            assert_eq!(repeated, None);
        }
        let last_seen: String =
            sqlx::query_scalar("SELECT last_seen FROM outages WHERE subject = ? AND cause = ?")
                .bind(subject)
                .bind(cause)
                .fetch_one(&pool)
                .await
                .expect("last_seen");
        assert_eq!(last_seen, "2026-10-28T04:00:00Z");

        let other = observe(&pool, subject, "2FA 코드 필요", true, at("2026-10-28T05:00:00Z"))
            .await
            .expect("new cause")
            .expect("a new cause is reported");
        assert_eq!(other.state, OutageState::Active);

        let recovered = observe(&pool, subject, "", false, at("2026-10-29T00:00:00Z"))
            .await
            .expect("recovery")
            .expect("recovery is reported");
        assert_eq!(recovered.state, OutageState::Recovered);
        assert!(recovered.body.contains("복구"));

        assert_eq!(
            observe(&pool, subject, "", false, at("2026-10-29T01:00:00Z"))
                .await
                .expect("still healthy"),
            None
        );

        let again = observe(&pool, subject, cause, true, at("2026-10-30T00:00:00Z"))
            .await
            .expect("relapse")
            .expect("a relapse after recovery is reported");
        assert_eq!(again.state, OutageState::Active);
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outages WHERE subject = ?")
            .bind(subject)
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(rows, 2);
    }

    #[tokio::test]
    async fn an_english_owner_reads_outage_notices_in_english() {
        crate::locale::scope(crate::locale::OwnerLocale::english_utc(), async {
            let pool = crate::storage::memory().await.expect("memory pool");
            let first = observe(&pool, "nightly", "disk full", true, at("2026-10-28T00:00:00Z"))
                .await
                .expect("failure")
                .expect("reported");
            assert_eq!(
                first.body,
                "nightly is failing. Cause: disk full. Please take care of it; I will tell you once more when it recovers."
            );
            let recovered = observe(&pool, "nightly", "", false, at("2026-10-28T01:00:00Z"))
                .await
                .expect("recovery")
                .expect("reported");
            assert_eq!(recovered.body, "nightly has recovered. (Previous cause: disk full)");
            assert!(!recovered.body.chars().any(crate::locale::is_hangul));
        })
        .await;
    }
}
