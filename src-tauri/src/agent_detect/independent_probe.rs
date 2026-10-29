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

#[test]
fn probe_plain_ls_la_output_returns_none() {
    // Guard against: all manifests (antigravity, cline, copilot, cursor, grok, kimi) falsely matching directory listing output.
    let rows = [
        "$ ls -la",
        "total 128",
        "drwxr-xr-x  14 indo staff   448 Aug 27 08:00 .",
        "drwxr-xr-x   6 indo staff   192 Aug 27 07:00 ..",
        "-rw-r--r--   1 indo staff  1024 Aug 27 07:15 Cargo.toml",
        "-rw-r--r--   1 indo staff 24500 Aug 27 07:45 Cargo.lock",
        "drwxr-xr-x   8 indo staff   256 Aug 27 07:30 src",
        "$ ",
    ];
    for title in [
        "",
        "zsh",
        "bash",
        "antigravity",
        "cline",
        "copilot",
        "cursor",
        "kimi",
    ] {
        assert_eq!(
            detect(&rows, title, None),
            None,
            "plain ls -la with title {title:?} must not fabricate state"
        );
    }
}

#[test]
fn probe_git_log_with_quoted_prompt_words_returns_none() {
    // Guard against: antigravity ("do you want to proceed"), kimi ("approve", "reject"), cursor, omo false-matching commit subjects.
    let rows = [
        "$ git log --oneline -4",
        "b97a6c2 fix: handle \"do you want to proceed\" prompt",
        "5ca3201 feat: add option to approve or reject pending requests",
        "8d4e21a docs: explain confirmation steps",
        "$ ",
    ];
    let got = detect(&rows, "", None);
    assert_eq!(
        got, None,
        "commit subjects quoting prompt words must not fabricate state; got {got:?}"
    );
}

#[test]
fn probe_dev_server_stop_hint_does_not_fabricate_working() {
    // Guard against: cursor. herdr ships `stop_hint_working` (contains "ctrl+c to stop"),
    // which is safe only when scoped to a known-cursor pane. Under Ferryx's global manifest
    // evaluation a long-running dev server parks that exact line at the bottom of the screen
    // indefinitely, so importing the rule marked unrelated panes as a working agent.
    // The rule is therefore dropped from cursor.toml; this probe is what proves it stays out.
    let rows = [
        "$ docker compose up",
        "Attaching to db_1, web_1",
        "db_1   | PostgreSQL Database directory appears to contain a database",
        "db_1   | 2026-08-27 12:00:00.000 UTC [1] LOG:  database system is ready to accept connections",
        "web_1  | Server listening on port 8080",
        "Press CTRL+C to stop",
    ];
    assert_eq!(
        detect(&rows, "", None),
        None,
        "a dev server's 'Press CTRL+C to stop' banner must not fabricate agent activity"
    );

    // Same phrasing from a static site server, which sits at this screen while idle.
    let hugo = [
        "Web Server is available at http://localhost:1313/",
        "Press Ctrl+C to stop",
    ];
    assert_eq!(detect(&hugo, "", None), None);
}

#[test]
fn probe_build_log_with_waiting_does_not_return_blocked() {
    // Guard against: omo, grok, cursor false-blocking on build logs containing 'waiting'.
    let rows = [
        "$ cargo build",
        "   Compiling ferryx v0.1.0 (/Users/example/code/project/orca-lite/src-tauri)",
        "waiting for connection to database...",
        "    Finished dev [unoptimized + debuginfo] target(s) in 2.34s",
        "$ ",
    ];
    let got = detect(&rows, "", None);
    assert_ne!(
        got,
        Some(AgentActivity::Blocked),
        "build log containing 'waiting' line must not be Blocked; got {got:?}"
    );
    assert_eq!(got, None);
}

#[test]
fn probe_bare_shell_prompt_empty_title_returns_none() {
    // Guard against: all manifests falsely matching a bare shell prompt with empty title.
    let rows = ["$ "];
    assert_eq!(
        detect(&rows, "", None),
        None,
        "bare shell prompt with empty title must return None"
    );
}

#[test]
fn probe_generic_selection_and_esc_cancel_do_not_fabricate_copilot() {
    // Guard against: copilot. selection_blocker ("enter to select", "esc to cancel")
    // and working_cancel_hint ("esc to cancel") falsely matching generic CLI prompts (fzf, inquirer, select).
    let select_rows = [
        "? Select a package to install: (Use arrow keys)",
        "❯ 1. react",
        "  2. vue",
        "enter to select · esc to cancel",
    ];
    assert_eq!(
        detect(&select_rows, "", None),
        None,
        "generic enter-to-select / esc-to-cancel prompt must not fabricate copilot"
    );

    let cancel_rows = ["$ fzf", "> search query", "esc to cancel"];
    assert_eq!(
        detect(&cancel_rows, "", None),
        None,
        "generic esc-to-cancel prompt must not fabricate copilot"
    );
}

// Measured 2026-09-28 on a live senpi (omo) pane that the UI kept reporting as working for 40
// minutes after its transcript went quiet: senpi draws no bare ">" prompt, so between turns
// its bottom lines are the status footer and the OmO/memory tag line.
const SENPI_IDLE_FOOTER: [&str; 4] = [
    "  Reloaded keybindings, extensions, skills, prompts, themes, and context files",
    "",
    "/Volumes/Data/Noveling/noveling \u{2022} main \u{2022} $0.552 \u{2022} 359K/667K (53.8%) (auto)      (kiro-lb) claude-opus-5.5:medium",
    "(\u{1f63a} OmO Native by Q Kim) mem:noveling-572243cf 10m ago (+53)",
];

#[test]
fn probe_senpi_idle_footer_settles_a_pane_last_seen_working() {
    assert_eq!(
        detect(&SENPI_IDLE_FOOTER, "OmO - noveling", Some(AgentActivity::Working)),
        Some(AgentActivity::Idle),
        "senpi's between-turn footer is idle evidence; holding the previous working state pins the pane forever"
    );
}

#[test]
fn probe_senpi_live_working_line_above_footer_stays_working() {
    let rows = [
        "  \u{2022} Working (18s \u{2022} esc to interrupt)",
        "/Volumes/Data/project/ferryx \u{2022} main \u{2022} $1.204 \u{2022} 120K/667K (18.0%) (auto)      claude-opus-5.5:medium",
        "(\u{1f63a} OmO Native by Q Kim) mem:ferryx-f3088a01 just now (+20)",
    ];
    assert_eq!(
        detect(&rows, "OmO - ferryx", Some(AgentActivity::Idle)),
        Some(AgentActivity::Working),
        "the live working line directly above the footer must veto the footer idle rule"
    );
}

#[test]
fn probe_senpi_ask_user_question_reaches_blocked() {
    let rows = [
        "  \u{2022} Waiting for your answer",
        "  Approach \u{2014} Which display should tabs use?",
        "  \u{276f} 1. Live agent title",
        "    2. Branch name",
        "  Submit (0/1 answered) \u{2014} Enter advances",
        "(\u{1f63a} OmO Native by Q Kim) mem:ferryx-f3088a01 just now (+20)",
    ];
    assert_eq!(
        detect(&rows, "? Approach", Some(AgentActivity::Working)),
        Some(AgentActivity::Blocked),
        "senpi's question box is a request for the user; it must not read as working"
    );
}

#[test]
fn probe_quoted_waiting_for_answer_without_question_box_is_not_blocked() {
    let rows = [
        "$ grep -rn 'Waiting for your answer' src/",
        "src/ui.ts:12:  status = \"Waiting for your answer\";",
        "$ ",
    ];
    assert_ne!(
        detect(&rows, "zsh", None),
        Some(AgentActivity::Blocked),
        "quoting the phrase in a shell must not fabricate a pending question"
    );
}
