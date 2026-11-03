//! Learned answers and the escalation decision (plan D1, D3, G1).
//!
//! The supervisor answers a worker prompt on the owner's behalf from, in order:
//! 1. a standing rule whose `params.answers` names the question and the option label;
//! 2. the goal contract ([`super::answers::routine_answer`]);
//! 3. the owner's own past answers to the same prompt shape in the same repo and agent: the
//!    same choice at least [`MIN_PRECEDENTS`] times and never contradicted within the last
//!    [`HISTORY_WINDOW`] answers.
//!
//! Before any of that, the irreversible/external classifier ([`irreversible_or_external`],
//! D1a) runs: a non-local database migration, deploy, publish, release, forced push, `rm -rf`
//! outside the worktree, `DROP`/`DELETE FROM`, payment, credentials and permission expansion
//! always escalate, whatever a rule, the contract or the history says. A migration whose target
//! cannot be shown to be local (no database URL, or a remote one) counts as non-local.
//!
//! Everything else escalates with a recommended option and a one-line reason: the option the
//! owner's history favours, else the safest non-destructive one.
use sqlx::SqlitePool;

use super::answers::{
    routine_answer, AnswerDecision as ContractVerdict, PromptDecision, PromptKind, WorkerPrompt,
};
use super::contract::{AnswerDecision, PendingPrompt, RuleScope, StandingRule};
use crate::error::Result;

/// Same choice needed, at least this many times, before the supervisor answers for the owner.
pub const MIN_PRECEDENTS: usize = 3;
/// Past answers (newest first) that must all agree.
pub const HISTORY_WINDOW: usize = 10;

/// One owner answer to record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerRecord<'a> {
    pub prompt: &'a PendingPrompt,
    pub repo: &'a str,
    pub agent: &'a str,
    pub option_label: &'a str,
    pub at_ms: i64,
}

/// What the decision knows about the prompt's origin.
#[derive(Debug, Clone, Copy)]
pub struct AnswerContext<'a> {
    pub prompt: &'a PendingPrompt,
    pub repo: &'a str,
    pub agent: &'a str,
    pub host_id: &'a str,
    /// The session's worktree: `rm -rf` inside it is not an external step.
    pub worktree: Option<&'a str>,
}

/// The goal contract's decisions for the session's goal at `goal_revision`.
#[derive(Debug, Clone, Copy)]
pub struct ContractDecisions<'a> {
    pub goal_revision: i64,
    pub decisions: &'a [PromptDecision],
}

/// Persisted owner answers.
#[derive(Debug, Clone)]
pub struct AnswerHistory {
    pool: SqlitePool,
}

impl AnswerHistory {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Records one owner answer.
    pub async fn record(&self, entry: &AnswerRecord<'_>) -> Result<()> {
        sqlx::query(
            "INSERT INTO supervisor_answer_history(shape_key, repo, agent, option_label, at_ms)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(shape_key(entry.prompt))
        .bind(entry.repo)
        .bind(entry.agent)
        .bind(entry.option_label)
        .bind(entry.at_ms)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Chosen labels for this prompt shape in this repo and agent, newest first.
    pub async fn recent_labels(
        &self,
        prompt: &PendingPrompt,
        repo: &str,
        agent: &str,
    ) -> Result<Vec<String>> {
        let labels = sqlx::query_scalar::<_, String>(
            "SELECT option_label FROM supervisor_answer_history
             WHERE shape_key = ? AND repo = ? AND agent = ?
             ORDER BY id DESC LIMIT ?",
        )
        .bind(shape_key(prompt))
        .bind(repo)
        .bind(agent)
        .bind(i64::try_from(HISTORY_WINDOW).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await?;
        Ok(labels)
    }

    /// Reads the history for `ctx` and decides.
    pub async fn decide(
        &self,
        ctx: &AnswerContext<'_>,
        rules: &[StandingRule],
        contract: Option<ContractDecisions<'_>>,
    ) -> Result<AnswerDecision> {
        let history = self.recent_labels(ctx.prompt, ctx.repo, ctx.agent).await?;
        Ok(decide(ctx, rules, contract, &history))
    }
}

/// The answer-on-behalf decision. `history` holds the owner's past choices for this prompt
/// shape in this repo and agent, newest first.
pub fn decide(
    ctx: &AnswerContext<'_>,
    rules: &[StandingRule],
    contract: Option<ContractDecisions<'_>>,
    history: &[String],
) -> AnswerDecision {
    let options = &ctx.prompt.options;
    if let Some(category) = irreversible_or_external(ctx.prompt, ctx.worktree) {
        return escalate(
            options,
            history,
            format!("This is an irreversible or external step ({category}); it needs your decision."),
        );
    }
    if let Some((index, label)) = standing_rule_answer(ctx, rules) {
        return AnswerDecision::Answer {
            option_index: index,
            basis: format!("Standing rule: {label}"),
        };
    }
    if let Some(contract) = contract {
        let prompt = WorkerPrompt {
            prompt_id: ctx.prompt.prompt_id.clone(),
            kind: prompt_kind(&ctx.prompt.kind),
            screen_readable: true,
            options: options.clone(),
        };
        match routine_answer(&prompt, contract.goal_revision, contract.decisions) {
            ContractVerdict::Answer {
                option_index,
                option_label,
                ..
            } => {
                return AnswerDecision::Answer {
                    option_index: option_index as usize,
                    basis: format!("The goal contract says: {option_label}"),
                };
            }
            ContractVerdict::Owner {
                reason: super::answers::OwnerReason::ConflictingDecisions,
            } => {
                return escalate(
                    options,
                    history,
                    "The goal's decisions for this question disagree.".to_owned(),
                );
            }
            ContractVerdict::Owner { .. } => {}
        }
    }
    let window: Vec<&String> = history.iter().take(HISTORY_WINDOW).collect();
    let Some(newest) = window.first() else {
        return escalate(
            options,
            history,
            "No earlier answer of yours to this question.".to_owned(),
        );
    };
    if window.iter().any(|label| label != newest) {
        return escalate(
            options,
            history,
            "Your earlier answers to this question differ.".to_owned(),
        );
    }
    if window.len() < MIN_PRECEDENTS {
        return escalate(
            options,
            history,
            format!(
                "Only {} earlier answer(s) of yours to this question; {MIN_PRECEDENTS} needed.",
                window.len()
            ),
        );
    }
    match unique_index(options, newest) {
        Some(option_index) => AnswerDecision::Answer {
            option_index,
            basis: format!("You chose {newest} the last {} times", window.len()),
        },
        None => escalate(
            options,
            history,
            format!("You chose {newest} before, but it is not offered now."),
        ),
    }
}

fn escalate(options: &[String], history: &[String], reason: String) -> AnswerDecision {
    AnswerDecision::Escalate {
        recommended_index: recommended_index(options, history),
        reason,
    }
}

fn unique_index(options: &[String], label: &str) -> Option<usize> {
    let mut found = options
        .iter()
        .enumerate()
        .filter(|(_, option)| option.as_str() == label)
        .map(|(index, _)| index);
    match (found.next(), found.next()) {
        (Some(index), None) => Some(index),
        _ => None,
    }
}

fn prompt_kind(kind: &str) -> PromptKind {
    match kind {
        "permission" => PromptKind::Permission,
        "approval" => PromptKind::Approval,
        _ => PromptKind::Question,
    }
}

/// The option the history favours (most chosen among the window, the newest wins a tie),
/// else the safest non-destructive one, else the first.
fn recommended_index(options: &[String], history: &[String]) -> usize {
    let mut best: Option<(usize, usize)> = None;
    for label in history.iter().take(HISTORY_WINDOW) {
        let Some(index) = unique_index(options, label) else {
            continue;
        };
        let count = history
            .iter()
            .take(HISTORY_WINDOW)
            .filter(|other| *other == label)
            .count();
        if best.is_none_or(|(_, top)| count > top) {
            best = Some((index, count));
        }
    }
    if let Some((index, _)) = best {
        return index;
    }
    const SAFE: &[&str] = &[
        "deny", "no", "cancel", "skip", "decline", "reject", "abort", "stop", "keep", "later",
    ];
    const RISKY: &[&str] = &[
        "yes", "allow", "approve", "grant", "run", "apply", "delete", "remove", "overwrite",
        "force", "drop", "deploy", "publish", "migrate", "proceed", "continue",
    ];
    let tokens: Vec<Vec<String>> = options.iter().map(|option| words(option)).collect();
    let has = |index: usize, list: &[&str]| tokens[index].iter().any(|t| list.contains(&t.as_str()));
    (0..options.len())
        .find(|index| has(*index, SAFE))
        .or_else(|| (0..options.len()).find(|index| !has(*index, RISKY)))
        .unwrap_or(0)
}

/// A standing rule in scope whose `params.answers` (`[{"match": text, "option": label}]`) has
/// an entry whose `match` is in the question and whose `option` is exactly one option.
fn standing_rule_answer<'a>(
    ctx: &AnswerContext<'a>,
    rules: &'a [StandingRule],
) -> Option<(usize, &'a str)> {
    let question = ctx.prompt.question.to_lowercase();
    rules
        .iter()
        .filter(|rule| in_scope(&rule.scope, ctx))
        .filter_map(|rule| rule.params.get("answers")?.as_array())
        .flatten()
        .find_map(|entry| {
            let needle = entry.get("match")?.as_str()?.to_lowercase();
            let label = entry.get("option")?.as_str()?;
            if needle.is_empty() || !question.contains(&needle) {
                return None;
            }
            Some((unique_index(&ctx.prompt.options, label)?, label))
        })
}

fn in_scope(scope: &RuleScope, ctx: &AnswerContext<'_>) -> bool {
    let matches = |want: &Option<String>, have: &str| want.as_deref().is_none_or(|w| w == have);
    matches(&scope.repo, ctx.repo)
        && matches(&scope.host_id, ctx.host_id)
        && matches(&scope.agent, ctx.agent)
}

/// Prompt shape: the question with numbers and paths removed, lowercased, whitespace
/// collapsed, then the sorted, lowercased option set.
pub fn shape_key(prompt: &PendingPrompt) -> String {
    let question: Vec<String> = prompt
        .question
        .split_whitespace()
        .filter(|token| !is_path_like(token))
        .map(|token| token.chars().filter(|c| !c.is_ascii_digit()).collect::<String>())
        .filter(|token| !token.is_empty())
        .collect();
    let mut options: Vec<String> = prompt
        .options
        .iter()
        .map(|option| option.trim().to_lowercase())
        .collect();
    options.sort();
    format!("{}\n{}", question.join(" ").to_lowercase(), options.join("\u{1f}"))
}

fn is_path_like(token: &str) -> bool {
    let token = token.trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '(' | ')' | ',' | '?'));
    token.contains('/')
        || token.contains('\\')
        || token.starts_with('~')
        || token.starts_with("./")
        || (token.as_bytes().get(1) == Some(&b':') && token.len() > 2)
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// D1a. The category of an irreversible or external step the prompt (question, command or
/// options) mentions, or `None`. Such a prompt always goes to the owner.
pub fn irreversible_or_external(
    prompt: &PendingPrompt,
    worktree: Option<&str>,
) -> Option<&'static str> {
    let mut text = prompt.question.clone();
    if let Some(command) = &prompt.command {
        text.push('\n');
        text.push_str(command);
    }
    for option in &prompt.options {
        text.push('\n');
        text.push_str(option);
    }
    classify_text(&text, worktree)
}

fn classify_text(text: &str, worktree: Option<&str>) -> Option<&'static str> {
    let words = words(text);
    let any = |list: &[&str]| words.iter().any(|w| list.contains(&w.as_str()));
    let prefixed = |prefixes: &[&str]| {
        words
            .iter()
            .any(|w| prefixes.iter().any(|prefix| w.starts_with(prefix)))
    };
    let followed = |first: &[&str], second: &[&str]| {
        words
            .windows(2)
            .any(|pair| first.contains(&pair[0].as_str()) && second.contains(&pair[1].as_str()))
    };
    if migration_not_provably_local(text, &words) {
        return Some("database migration on a non-local database");
    }
    if prefixed(&["deploy"]) {
        return Some("deploy");
    }
    if prefixed(&["publish"]) {
        return Some("publish");
    }
    if prefixed(&["release"]) {
        return Some("release");
    }
    if (any(&["push"]) && any(&["force"]))
        || (any(&["push"]) && text.split_whitespace().any(|t| t == "-f"))
    {
        return Some("forced push");
    }
    if rm_outside_worktree(text, worktree) {
        return Some("recursive delete outside the worktree");
    }
    if followed(
        &["drop"],
        &[
            "table", "database", "schema", "index", "column", "view", "user", "role", "trigger",
        ],
    ) || followed(&["delete"], &["from"])
        || followed(&["truncate"], &["table"])
    {
        return Some("destructive database statement");
    }
    if prefixed(&["payment", "billing", "purchase", "invoice"]) || followed(&["credit"], &["card"])
    {
        return Some("payment");
    }
    if prefixed(&["credential", "secret", "token", "password", "passwd", "apikey", "api_key"])
        || any(&["id_rsa"])
        || followed(&["api", "private", "ssh"], &["key"])
    {
        return Some("credentials or secrets");
    }
    if any(&["sudo", "doas", "chmod", "chown", "chgrp", "setfacl", "visudo", "usermod"])
        || prefixed(&["permission", "privilege"])
    {
        return Some("permission expansion");
    }
    None
}

/// A migration is fine only when every database URL it mentions is local (`sqlite:`,
/// `file:`, localhost, 127.0.0.1, ::1). With no URL at all the target cannot be shown to be
/// local, so it counts as non-local.
fn migration_not_provably_local(text: &str, words: &[String]) -> bool {
    if !words.iter().any(|w| w.starts_with("migrat")) {
        return false;
    }
    let mut seen_url = false;
    for token in text.split_whitespace() {
        let token = token.trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | ',' | ';' | '(' | ')'));
        let lower = token.to_lowercase();
        if let Some(at) = lower.find("://") {
            seen_url = true;
            let scheme = lower[..at].rsplit('=').next().unwrap_or("");
            if scheme == "sqlite" || scheme == "file" {
                continue;
            }
            let rest = &lower[at + 3..];
            let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
            let host_port = authority.rsplit('@').next().unwrap_or("");
            let host = match host_port.strip_prefix('[') {
                Some(inner) => inner.split(']').next().unwrap_or(""),
                None => host_port.split(':').next().unwrap_or(""),
            };
            if !matches!(host, "localhost" | "127.0.0.1" | "::1" | "0.0.0.0") {
                return true;
            }
        } else if lower.starts_with("sqlite:") || lower.starts_with("file:") {
            seen_url = true;
        }
    }
    !seen_url
}

fn rm_outside_worktree(text: &str, worktree: Option<&str>) -> bool {
    let tokens: Vec<&str> = text
        .split_whitespace()
        .map(|t| t.trim_matches(|c: char| matches!(c, '"' | '\'' | '`')))
        .collect();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] != "rm" {
            i += 1;
            continue;
        }
        let mut recursive = false;
        let mut targets: Vec<&str> = Vec::new();
        i += 1;
        while i < tokens.len() {
            let (token, ends) = match tokens[i].strip_suffix(';') {
                Some(stripped) => (stripped, true),
                None => (tokens[i], false),
            };
            if matches!(token, "" | "&&" | "||" | "|") {
                break;
            }
            if let Some(long) = token.strip_prefix("--") {
                recursive |= long == "recursive";
            } else if let Some(short) = token.strip_prefix('-') {
                recursive |= short.chars().any(|c| matches!(c, 'r' | 'R'));
            } else {
                targets.push(token);
            }
            i += 1;
            if ends {
                break;
            }
        }
        if recursive && (targets.is_empty() || targets.iter().any(|t| !inside_worktree(t, worktree)))
        {
            return true;
        }
    }
    false
}

fn inside_worktree(target: &str, worktree: Option<&str>) -> bool {
    if target.contains("..") || target.starts_with('~') || target.starts_with('$') {
        return false;
    }
    let absolute = target.starts_with('/')
        || target.starts_with('\\')
        || target.as_bytes().get(1) == Some(&b':');
    if !absolute {
        return true;
    }
    let Some(root) = worktree.map(|w| w.trim_end_matches(['/', '\\'])) else {
        return false;
    };
    !root.is_empty()
        && target
            .strip_prefix(root)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::supervisor::answers::DecisionSource;
    use crate::supervisor::contract::RuleKind;

    fn prompt(question: &str, command: Option<&str>, options: &[&str]) -> PendingPrompt {
        PendingPrompt {
            prompt_id: "p1".into(),
            kind: "question".into(),
            question: question.into(),
            command: command.map(str::to_owned),
            options: options.iter().map(|o| (*o).to_owned()).collect(),
            screen_revision: Some("r1".into()),
        }
    }

    fn ctx(prompt: &PendingPrompt) -> AnswerContext<'_> {
        AnswerContext {
            prompt,
            repo: "ferryx",
            agent: "claude",
            host_id: "mac",
            worktree: Some("/work/ferryx"),
        }
    }

    fn history(label: &str, times: usize) -> Vec<String> {
        vec![label.to_owned(); times]
    }

    fn layout() -> PendingPrompt {
        prompt("Which layout for the settings page?", None, &["Tabs", "Single page"])
    }

    #[test]
    fn three_identical_choices_answer_with_basis() {
        let p = layout();
        assert_eq!(
            decide(&ctx(&p), &[], None, &history("Single page", 3)),
            AnswerDecision::Answer {
                option_index: 1,
                basis: "You chose Single page the last 3 times".into()
            }
        );
    }

    #[test]
    fn two_choices_or_none_escalate_with_the_favoured_option() {
        let p = layout();
        assert_eq!(
            decide(&ctx(&p), &[], None, &history("Single page", 2)),
            AnswerDecision::Escalate {
                recommended_index: 1,
                reason: "Only 2 earlier answer(s) of yours to this question; 3 needed.".into()
            }
        );
        assert_eq!(
            decide(&ctx(&p), &[], None, &[]),
            AnswerDecision::Escalate {
                recommended_index: 0,
                reason: "No earlier answer of yours to this question.".into()
            }
        );
    }

    #[test]
    fn contradicted_history_escalates() {
        let p = layout();
        let mut past = history("Single page", 4);
        past.insert(2, "Tabs".into());
        assert!(matches!(
            decide(&ctx(&p), &[], None, &past),
            AnswerDecision::Escalate {
                recommended_index: 1,
                ..
            }
        ));
    }

    #[test]
    fn contradiction_older_than_the_window_does_not_block() {
        let p = layout();
        let mut past = history("Single page", HISTORY_WINDOW);
        past.push("Tabs".into());
        assert!(matches!(
            decide(&ctx(&p), &[], None, &past),
            AnswerDecision::Answer { option_index: 1, .. }
        ));
    }

    #[test]
    fn prod_migration_escalates_even_with_history() {
        let p = prompt(
            "Run the migration?",
            Some("sqlx migrate run --database-url postgres://prod.example.com/app"),
            &["Allow", "Deny"],
        );
        let decision = decide(&ctx(&p), &[], None, &history("Allow", 5));
        assert!(
            matches!(decision, AnswerDecision::Escalate { recommended_index: 0, .. }),
            "{decision:?}"
        );
    }

    #[test]
    fn local_migration_with_history_answers() {
        for url in ["postgres://user:pw@localhost:5432/app", "sqlite://hub.db", "file:hub.db"] {
            let command = format!("sqlx migrate run --database-url {url}");
            let p = prompt("Run the migration?", Some(&command), &["Allow", "Deny"]);
            assert_eq!(
                decide(&ctx(&p), &[], None, &history("Allow", 3)),
                AnswerDecision::Answer {
                    option_index: 0,
                    basis: "You chose Allow the last 3 times".into()
                },
                "{url}"
            );
        }
    }

    #[test]
    fn migration_without_a_url_is_not_provably_local() {
        let p = prompt("Run the migration?", None, &["Allow", "Deny"]);
        assert!(matches!(
            decide(&ctx(&p), &[], None, &history("Allow", 5)),
            AnswerDecision::Escalate { .. }
        ));
    }

    #[test]
    fn d1a_items_always_escalate_and_recommend_the_safe_option() {
        let cases = [
            ("Deploy to staging?", None),
            ("Publish the crate?", None),
            ("Cut a release?", None),
            ("Run it?", Some("git push --force origin main")),
            ("Run it?", Some("git push -f origin main")),
            ("Run it?", Some("rm -rf /var/lib/app")),
            ("Run it?", Some("rm -rf /work/ferryx-other/build")),
            ("Run it?", Some("rm -rf ../elsewhere")),
            ("Run it?", Some("psql -c 'DROP TABLE users'")),
            ("Run it?", Some("psql -c 'delete from users'")),
            ("Charge the payment method?", None),
            ("Paste the API token?", None),
            ("Run it?", Some("sudo make install")),
        ];
        for (question, command) in cases {
            let p = prompt(question, command, &["Allow", "Deny"]);
            assert_eq!(
                decide(&ctx(&p), &[], None, &[]),
                AnswerDecision::Escalate {
                    recommended_index: 1,
                    reason: match irreversible_or_external(&p, Some("/work/ferryx")) {
                        Some(category) => format!(
                            "This is an irreversible or external step ({category}); it needs your decision."
                        ),
                        None => panic!("not classified: {question} {command:?}"),
                    }
                }
            );
            assert!(matches!(
                decide(&ctx(&p), &[], None, &history("Allow", 9)),
                AnswerDecision::Escalate { .. }
            ));
        }
    }

    #[test]
    fn rm_inside_the_worktree_is_routine() {
        for command in ["rm -rf target", "rm -rf /work/ferryx/target", "rm -f /etc/hosts"] {
            let p = prompt("Clean up?", Some(command), &["Yes", "No"]);
            assert_eq!(
                irreversible_or_external(&p, Some("/work/ferryx")),
                None,
                "{command}"
            );
        }
    }

    #[test]
    fn d1a_beats_rule_and_contract() {
        let p = prompt("Deploy to staging?", None, &["Yes", "No"]);
        let rule = StandingRule {
            id: 1,
            scope: RuleScope::default(),
            kind: RuleKind::AskBeforeIrreversible,
            params: json!({ "answers": [{ "match": "deploy", "option": "Yes" }] }),
            created_from_message_id: None,
            created_at_ms: 0,
        };
        let decisions = [PromptDecision {
            goal_revision: 1,
            prompt_id: "p1".into(),
            option_label: "Yes".into(),
            source: DecisionSource::Owner,
        }];
        let contract = ContractDecisions {
            goal_revision: 1,
            decisions: &decisions,
        };
        assert!(matches!(
            decide(&ctx(&p), &[rule], Some(contract), &history("Yes", 5)),
            AnswerDecision::Escalate { .. }
        ));
    }

    #[test]
    fn standing_rule_then_contract_then_history() {
        let p = layout();
        let rule = |repo: Option<&str>| StandingRule {
            id: 1,
            scope: RuleScope {
                repo: repo.map(str::to_owned),
                host_id: None,
                agent: None,
            },
            kind: RuleKind::UpstreamRetry,
            params: json!({ "answers": [{ "match": "layout", "option": "Tabs" }] }),
            created_from_message_id: None,
            created_at_ms: 0,
        };
        let decisions = [PromptDecision {
            goal_revision: 2,
            prompt_id: "p1".into(),
            option_label: "Single page".into(),
            source: DecisionSource::Contract,
        }];
        let contract = ContractDecisions {
            goal_revision: 2,
            decisions: &decisions,
        };
        let past = history("Single page", 3);
        assert_eq!(
            decide(&ctx(&p), &[rule(Some("ferryx"))], Some(contract), &past),
            AnswerDecision::Answer {
                option_index: 0,
                basis: "Standing rule: Tabs".into()
            }
        );
        // A rule for another repo does not apply; the contract answers.
        assert_eq!(
            decide(&ctx(&p), &[rule(Some("other"))], Some(contract), &[]),
            AnswerDecision::Answer {
                option_index: 1,
                basis: "The goal contract says: Single page".into()
            }
        );
    }

    #[test]
    fn shape_ignores_numbers_and_paths_but_not_options() {
        let a = prompt("Rename src/a.rs to 3 files?", None, &["Yes", "No"]);
        let b = prompt("Rename ~/x/b.rs to 12 files?", None, &["No", "Yes"]);
        let c = prompt("Rename src/a.rs to 3 files?", None, &["Yes", "No", "Later"]);
        assert_eq!(shape_key(&a), shape_key(&b));
        assert_ne!(shape_key(&a), shape_key(&c));
    }

    #[tokio::test]
    async fn stored_answers_drive_the_decision_per_repo_and_agent() {
        let store = AnswerHistory::new(crate::storage::memory().await.unwrap());
        let p = layout();
        for at_ms in 0..3 {
            store
                .record(&AnswerRecord {
                    prompt: &p,
                    repo: "ferryx",
                    agent: "claude",
                    option_label: "Single page",
                    at_ms,
                })
                .await
                .unwrap();
        }
        assert!(matches!(
            store.decide(&ctx(&p), &[], None).await.unwrap(),
            AnswerDecision::Answer { option_index: 1, .. }
        ));
        let other_agent = AnswerContext {
            agent: "codex",
            ..ctx(&p)
        };
        assert!(matches!(
            store.decide(&other_agent, &[], None).await.unwrap(),
            AnswerDecision::Escalate { .. }
        ));
    }
}
