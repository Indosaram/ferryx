use super::engine::{default_engine, AgentActivity};
use super::region::ScreenInput;

fn screen(rows: &[&str], title: &str) -> ScreenInput {
    ScreenInput {
        rows: rows.iter().map(|s| s.to_string()).collect(),
        title: title.to_string(),
    }
}

fn detect(rows: &[&str], title: &str, prev: Option<AgentActivity>) -> Option<AgentActivity> {
    default_engine()
        .detect(&screen(rows, title), prev)
        .map(|d| d.state)
}

#[test]
fn probe_measured_bare_titles_alone_do_not_fabricate_state() {
    for title in ["OmO - orca-lite", "OpenCode", "orca-lite"] {
        assert_eq!(
            detect(&["$ "], title, None),
            None,
            "bare title {title:?} must not invent a state without screen evidence"
        );
    }
}

#[test]
fn probe_working_screen_reaches_working_for_agents_with_no_status_title() {
    let rows = [
        "> refactor the activity reducer",
        "",
        "  Thinking...",
        "  esc to interrupt",
    ];
    assert_eq!(
        detect(&rows, "OmO - orca-lite", None),
        Some(AgentActivity::Working),
        "omo emits no status word in its title; the screen must carry it"
    );
    assert_eq!(
        detect(&rows, "OpenCode", None),
        Some(AgentActivity::Working)
    );
    assert_eq!(
        detect(&rows, "orca-lite", None),
        Some(AgentActivity::Working)
    );
}

#[test]
fn probe_permission_screen_reaches_blocked() {
    let rows = [
        "  Run this command?",
        "  rm -rf build/",
        "",
        "  Do you want to proceed?",
        "  1. Yes  2. No",
    ];
    assert_eq!(
        detect(&rows, "OmO - orca-lite", Some(AgentActivity::Working)),
        Some(AgentActivity::Blocked)
    );
}

#[test]
fn probe_ordinary_shell_output_does_not_fabricate_working() {
    let rows = [
        "$ ls -la",
        "total 64",
        "drwxr-xr-x  12 indo staff  384 Aug 27 07:00 .",
        "-rw-r--r--   1 indo staff 9304 Aug 27 07:30 README.md",
        "$ ",
    ];
    assert_eq!(
        detect(&rows, "orca-lite", None),
        None,
        "a plain shell listing must not trip any rule (global manifest evaluation risk)"
    );
}

#[test]
fn probe_working_then_unclassifiable_screen_holds_working() {
    let working = ["  esc to interrupt"];
    let state = detect(&working, "orca-lite", None);
    assert_eq!(state, Some(AgentActivity::Working));

    let noise = [
        "  building module 42/93",
        "  compiled ui/src/state/store.ts",
    ];
    assert_eq!(
        detect(&noise, "orca-lite", state),
        Some(AgentActivity::Working),
        "absence of evidence must hold, never reset"
    );
}

#[test]
fn probe_omo_real_permission_choices_reach_blocked() {
    let rows = [
        "  omo wants to run a command",
        "    rm -rf build/",
        "",
        "  > Allow once",
        "    Allow always",
        "    Deny with feedback",
    ];
    assert_eq!(
        detect(&rows, "OmO - orca-lite", Some(AgentActivity::Working)),
        Some(AgentActivity::Blocked),
        "omo's real permission UI renders Allow once / Allow always / Deny with feedback"
    );
}

#[test]
fn probe_git_log_mentioning_prompt_words_does_not_fabricate_blocked() {
    let rows = [
        "$ git log --oneline -3",
        "b97a6c2 fix: ask do you want to proceed before deleting",
        "5ca3201 feat: approve flow",
        "$ ",
    ];
    let got = detect(&rows, "orca-lite", None);
    assert_eq!(
        got, None,
        "commit subjects quoting prompt copy must not read as a live blocker; got {got:?}"
    );
}
