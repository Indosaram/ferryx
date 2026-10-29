pub mod engine;
pub mod manifest;
pub mod matcher;
pub mod region;

pub use engine::{default_engine, AgentActivity, Detection, DetectionEngine, SHIPPED_MANIFESTS};
pub use manifest::{
    load_manifest_from_str, CompiledManifest, CompiledRule, ManifestError, RawManifest, RawRule,
    RuleState,
};
pub use matcher::{CompiledMatcherNode, MatcherError, MatcherNode};
pub use region::{Region, RegionError, RegionView, ScreenInput};

#[cfg(test)]
mod tests {
    use super::*;

    // 1. priority ordering — two rules match, higher priority wins
    #[test]
    fn test_priority_ordering_higher_priority_wins() {
        let manifest_toml = r#"
id = "test_priority"
version = "1.0.0"

[[rules]]
id = "low_rule"
state = "idle"
priority = 50
region = "whole_recent"
contains = ["keyword"]

[[rules]]
id = "high_rule"
state = "working"
priority = 100
region = "whole_recent"
contains = ["keyword"]
"#;
        let manifest = load_manifest_from_str(manifest_toml).unwrap();
        let engine = DetectionEngine::new(vec![manifest]);
        let input = ScreenInput {
            rows: vec!["some line with keyword here".to_string()],
            title: "".to_string(),
        };
        let detection = engine.detect(&input, None).expect("should match");
        assert_eq!(detection.state, AgentActivity::Working);
        assert_eq!(detection.rule_id, "high_rule");
    }

    // 2. region scoping — a phrase outside bottom_non_empty_lines(3) does NOT match
    #[test]
    fn test_region_scoping_bottom_lines() {
        let manifest_toml = r#"
id = "test_region"
version = "1.0.0"

[[rules]]
id = "bottom_rule"
state = "working"
priority = 100
region = "bottom_non_empty_lines(3)"
contains = ["needle"]
"#;
        let manifest = load_manifest_from_str(manifest_toml).unwrap();
        let engine = DetectionEngine::new(vec![manifest]);

        // needle is at row 0, followed by 3 non-empty lines -> needle is outside bottom 3 non-empty lines
        let input_outside = ScreenInput {
            rows: vec![
                "needle here".to_string(),
                "line 1".to_string(),
                "line 2".to_string(),
                "line 3".to_string(),
                "".to_string(),
            ],
            title: "".to_string(),
        };
        let detection_outside = engine.detect(&input_outside, None);
        assert_eq!(detection_outside, None);

        // needle is inside bottom 3 non-empty lines
        let input_inside = ScreenInput {
            rows: vec![
                "line 1".to_string(),
                "needle here".to_string(),
                "line 2".to_string(),
                "".to_string(),
            ],
            title: "".to_string(),
        };
        let detection_inside = engine.detect(&input_inside, None).expect("should match");
        assert_eq!(detection_inside.state, AgentActivity::Working);
        assert_eq!(detection_inside.rule_id, "bottom_rule");
    }

    // 3. not veto — a rule that would match is suppressed by its not child
    #[test]
    fn test_not_veto_suppresses_match() {
        let manifest_toml = r#"
id = "test_not_veto"
version = "1.0.0"

[[rules]]
id = "vetoed_rule"
state = "working"
priority = 100
region = "whole_recent"
contains = ["working"]
not = [
  { contains = ["interrupted"] }
]
"#;
        let manifest = load_manifest_from_str(manifest_toml).unwrap();
        let engine = DetectionEngine::new(vec![manifest]);

        let input_suppressed = ScreenInput {
            rows: vec!["working on task... interrupted by user".to_string()],
            title: "".to_string(),
        };
        assert_eq!(engine.detect(&input_suppressed, None), None);

        let input_allowed = ScreenInput {
            rows: vec!["working on task...".to_string()],
            title: "".to_string(),
        };
        let detection = engine.detect(&input_allowed, None).expect("should match");
        assert_eq!(detection.state, AgentActivity::Working);
        assert_eq!(detection.rule_id, "vetoed_rule");
    }

    // 4. nested any/all composition
    #[test]
    fn test_nested_any_all_composition() {
        let manifest_toml = r#"
id = "test_composition"
version = "1.0.0"

[[rules]]
id = "nested_rule"
state = "blocked"
priority = 100
region = "whole_recent"
contains = ["prompt"]
all = [
  { any = [{ contains = ["yes"] }, { contains = ["confirm"] }] },
  { any = [{ contains = ["no"] }, { contains = ["cancel"] }] }
]
"#;
        let manifest = load_manifest_from_str(manifest_toml).unwrap();
        let engine = DetectionEngine::new(vec![manifest]);

        // Has prompt + yes + cancel -> matches
        let input_matching = ScreenInput {
            rows: vec!["prompt: [yes] / [cancel]".to_string()],
            title: "".to_string(),
        };
        let detection = engine.detect(&input_matching, None).expect("should match");
        assert_eq!(detection.state, AgentActivity::Blocked);
        assert_eq!(detection.rule_id, "nested_rule");

        // Has prompt + yes but no no/cancel -> fails
        let input_failing = ScreenInput {
            rows: vec!["prompt: [yes] only".to_string()],
            title: "".to_string(),
        };
        assert_eq!(engine.detect(&input_failing, None), None);
    }

    // 5. skip_state_update HOLDS the previous state (feed a transcript-viewer screen while previous is Working, assert the result is still Working)
    #[test]
    fn test_skip_state_update_holds_previous_state() {
        let engine = default_engine();

        let transcript_input = ScreenInput {
            rows: vec![
                "showing detailed transcript".to_string(),
                "ctrl+o to toggle".to_string(),
                "ctrl+e show all".to_string(),
            ],
            title: "".to_string(),
        };

        let detection = engine.detect(&transcript_input, Some(AgentActivity::Working));
        let unwrapped = detection.expect("detection should hold previous state");
        assert_eq!(unwrapped.state, AgentActivity::Working);
        assert_eq!(unwrapped.rule_id, "transcript_viewer");
    }

    // 6. no-match HOLDS the previous state
    #[test]
    fn test_no_match_holds_previous_state() {
        let engine = default_engine();

        let unclassifiable_input = ScreenInput {
            rows: vec![
                "random unrelated line 1".to_string(),
                "random unrelated line 2".to_string(),
            ],
            title: "zsh".to_string(),
        };

        // If previous is Working, detection holds Working
        let detection_working = engine.detect(&unclassifiable_input, Some(AgentActivity::Working));
        assert_eq!(
            detection_working,
            Some(Detection {
                state: AgentActivity::Working,
                rule_id: "".to_string(),
                manifest_id: "".to_string(),
            })
        );

        // If previous is Blocked, detection holds Blocked
        let detection_blocked = engine.detect(&unclassifiable_input, Some(AgentActivity::Blocked));
        assert_eq!(
            detection_blocked,
            Some(Detection {
                state: AgentActivity::Blocked,
                rule_id: "".to_string(),
                manifest_id: "".to_string(),
            })
        );

        // If previous is None, detection is None
        let detection_none = engine.detect(&unclassifiable_input, None);
        assert_eq!(detection_none, None);
    }

    // 7. every shipped manifest loads and validates (regions known, regexes compile)
    #[test]
    fn test_every_shipped_manifest_loads_and_validates() {
        for manifest_raw in SHIPPED_MANIFESTS {
            let manifest = load_manifest_from_str(manifest_raw).expect("manifest must parse");
            assert!(!manifest.id.is_empty());
            assert!(!manifest.rules.is_empty());
            for rule in &manifest.rules {
                assert!(!rule.id.is_empty());
                assert!(rule.priority > 0);
            }
        }
    }

    // 8. per-agent realistic screens: for omo, opencode, codex, claude, antigravity —
    // a working screen, a blocked/permission screen, and an idle screen each reach the expected state
    #[test]
    fn test_per_agent_realistic_screens() {
        let engine = default_engine();

        // --- OMO ---
        let omo_working = ScreenInput {
            rows: vec![
                "⠋ Thinking through algorithm approach...".to_string(),
                "esc to interrupt".to_string(),
            ],
            title: "OmO - orca-lite".to_string(),
        };
        assert_eq!(
            engine.detect(&omo_working, None).map(|d| d.state),
            Some(AgentActivity::Working)
        );

        let omo_blocked = ScreenInput {
            rows: vec![
                "I will execute cargo test.".to_string(),
                "waiting for approval to proceed".to_string(),
            ],
            title: "OmO - orca-lite".to_string(),
        };
        assert_eq!(
            engine.detect(&omo_blocked, None).map(|d| d.state),
            Some(AgentActivity::Blocked)
        );

        let omo_idle = ScreenInput {
            rows: vec![
                "Previous task completed successfully.".to_string(),
                "> ".to_string(),
            ],
            title: "OmO - orca-lite".to_string(),
        };
        assert_eq!(
            engine.detect(&omo_idle, None).map(|d| d.state),
            Some(AgentActivity::Idle)
        );

        // --- OPENCODE ---
        let opencode_working = ScreenInput {
            rows: vec![
                "Compiling source files...".to_string(),
                "esc to interrupt".to_string(),
            ],
            title: "OpenCode".to_string(),
        };
        assert_eq!(
            engine.detect(&opencode_working, None).map(|d| d.state),
            Some(AgentActivity::Working)
        );

        let opencode_blocked = ScreenInput {
            rows: vec![
                "Permission required: run bash script".to_string(),
                "esc dismiss / enter confirm".to_string(),
            ],
            title: "OpenCode".to_string(),
        };
        assert_eq!(
            engine.detect(&opencode_blocked, None).map(|d| d.state),
            Some(AgentActivity::Blocked)
        );

        let opencode_idle = ScreenInput {
            rows: vec![
                "Welcome to OpenCode".to_string(),
                "Ask anything... ctrl+p commands".to_string(),
            ],
            title: "OpenCode".to_string(),
        };
        assert_eq!(
            engine.detect(&opencode_idle, None).map(|d| d.state),
            Some(AgentActivity::Idle)
        );

        // --- CODEX ---
        let codex_working = ScreenInput {
            rows: vec!["Working (esc to interrupt)".to_string()],
            title: "orca-lite".to_string(),
        };
        assert_eq!(
            engine.detect(&codex_working, None).map(|d| d.state),
            Some(AgentActivity::Working)
        );

        let codex_blocked = ScreenInput {
            rows: vec![
                "Action Required: allow command?".to_string(),
                "press enter to confirm or esc to cancel".to_string(),
            ],
            title: "orca-lite".to_string(),
        };
        assert_eq!(
            engine.detect(&codex_blocked, None).map(|d| d.state),
            Some(AgentActivity::Blocked)
        );

        let codex_idle = ScreenInput {
            rows: vec![
                "Codex ready".to_string(),
                "Ask Codex anything".to_string(),
                "> ".to_string(),
            ],
            title: "orca-lite".to_string(),
        };
        assert_eq!(
            engine.detect(&codex_idle, None).map(|d| d.state),
            Some(AgentActivity::Idle)
        );

        // --- CLAUDE ---
        let claude_working = ScreenInput {
            rows: vec!["/btw checking logs".to_string(), "esc to close".to_string()],
            title: "⠋ claude".to_string(),
        };
        assert_eq!(
            engine.detect(&claude_working, None).map(|d| d.state),
            Some(AgentActivity::Working)
        );

        let claude_blocked = ScreenInput {
            rows: vec![
                "do you want to proceed?".to_string(),
                "1. yes".to_string(),
                "2. no".to_string(),
            ],
            title: "claude".to_string(),
        };
        assert_eq!(
            engine.detect(&claude_blocked, None).map(|d| d.state),
            Some(AgentActivity::Blocked)
        );

        let claude_idle = ScreenInput {
            rows: vec!["Ready for next instruction.".to_string()],
            title: "✳ claude".to_string(),
        };
        assert_eq!(
            engine.detect(&claude_idle, None).map(|d| d.state),
            Some(AgentActivity::Idle)
        );

        // --- ANTIGRAVITY (AGY) ---
        let agy_working = ScreenInput {
            rows: vec![
                "⠋ Analyzing repository structure...".to_string(),
                "Scanning dependencies...".to_string(),
            ],
            title: "antigravity".to_string(),
        };
        assert_eq!(
            engine.detect(&agy_working, None).map(|d| d.state),
            Some(AgentActivity::Working)
        );

        let agy_blocked = ScreenInput {
            rows: vec![
                "requesting permission for: rm -rf target/".to_string(),
                "do you want to proceed?".to_string(),
                "[y]es / [n]o".to_string(),
            ],
            title: "antigravity".to_string(),
        };
        assert_eq!(
            engine.detect(&agy_blocked, None).map(|d| d.state),
            Some(AgentActivity::Blocked)
        );

        // --- CLINE ---
        // Synthetic tool_permission screen; the measured cline screens are in parity_screens.
        let cline_blocked = ScreenInput {
            rows: vec![
                "Cline wants to execute a terminal command:".to_string(),
                "let cline use this tool".to_string(),
                "1. Approve  2. Reject".to_string(),
            ],
            title: "cline".to_string(),
        };
        assert_eq!(
            engine.detect(&cline_blocked, None).map(|d| d.state),
            Some(AgentActivity::Blocked)
        );

        // --- COPILOT ---
        let copilot_working = ScreenInput {
            rows: vec![
                "Generating suggestions for optimization...".to_string(),
                "esc again to cancel".to_string(),
            ],
            title: "copilot".to_string(),
        };
        assert_eq!(
            engine.detect(&copilot_working, None).map(|d| d.state),
            Some(AgentActivity::Working)
        );

        // Path 1: legacy selection_blocker
        let copilot_blocked_selection = ScreenInput {
            rows: vec![
                "Select action:".to_string(),
                "❯ 1. Apply suggested edits".to_string(),
                "esc to cancel · enter to select".to_string(),
            ],
            title: "copilot".to_string(),
        };
        assert_eq!(
            engine
                .detect(&copilot_blocked_selection, None)
                .map(|d| d.state),
            Some(AgentActivity::Blocked)
        );

        // Path 2: numeric_confirm_footer_blocked (real capture of Copilot CLI 0.0.375)
        let copilot_blocked_numeric = ScreenInput {
            rows: vec![
                "Do you trust the files in this folder?".to_string(),
                "❯ 1. Yes".to_string(),
                "  2. No".to_string(),
                "Confirm with number keys or ↑↓ keys and Enter, Cancel with Esc".to_string(),
            ],
            title: "copilot".to_string(),
        };
        assert_eq!(
            engine
                .detect(&copilot_blocked_numeric, None)
                .map(|d| d.state),
            Some(AgentActivity::Blocked)
        );

        // --- CURSOR ---
        // NOTE: herdr's `stop_hint_working` ("ctrl+c to stop") is deliberately NOT shipped -
        // it fabricated Working on any dev-server pane under global evaluation. Cursor's
        // working coverage comes from spinner_working / background_task_status_working.
        let cursor_working = ScreenInput {
            rows: vec![
                "Updating file src/agent_detect/mod.rs...".to_string(),
                "⬢ Editing files".to_string(),
            ],
            title: "cursor".to_string(),
        };
        assert_eq!(
            engine.detect(&cursor_working, None).map(|d| d.state),
            Some(AgentActivity::Working)
        );

        let cursor_blocked = ScreenInput {
            rows: vec![
                "Cursor proposing file changes:".to_string(),
                "write to this file?".to_string(),
                "proceed (y)".to_string(),
                "reject & propose changes".to_string(),
            ],
            title: "cursor".to_string(),
        };
        assert_eq!(
            engine.detect(&cursor_blocked, None).map(|d| d.state),
            Some(AgentActivity::Blocked)
        );

        // --- GROK ---
        let grok_working = ScreenInput {
            rows: vec!["⠧ Waiting on subagent… 2.8s   13s ⇣29.7k [stop]".to_string()],
            title: "⠧ grok".to_string(),
        };
        assert_eq!(
            engine.detect(&grok_working, None).map(|d| d.state),
            Some(AgentActivity::Working)
        );

        let grok_blocked = ScreenInput {
            rows: vec![
                "┃  1 (○) Yes, proceed".to_string(),
                "┃  2 (○) No, cancel".to_string(),
                "1/3:select │ Ctrl+o:yolo │ Ctrl+c:cancel".to_string(),
            ],
            title: "grok".to_string(),
        };
        assert_eq!(
            engine.detect(&grok_blocked, None).map(|d| d.state),
            Some(AgentActivity::Blocked)
        );

        let grok_idle = ScreenInput {
            rows: vec![
                "Grok Build ready for input".to_string(),
                "ctrl+.:shortcuts".to_string(),
            ],
            title: "grok".to_string(),
        };
        assert_eq!(
            engine.detect(&grok_idle, None).map(|d| d.state),
            Some(AgentActivity::Idle)
        );

        // --- KIMI ---
        let kimi_working = ScreenInput {
            rows: vec!["⠋ thinking...".to_string()],
            title: "kimi".to_string(),
        };
        assert_eq!(
            engine.detect(&kimi_working, None).map(|d| d.state),
            Some(AgentActivity::Working)
        );

        let kimi_blocked = ScreenInput {
            rows: vec![
                "run this command? cargo check".to_string(),
                "1/2 choose".to_string(),
                "approve".to_string(),
                "↵ confirm".to_string(),
            ],
            title: "kimi".to_string(),
        };
        assert_eq!(
            engine.detect(&kimi_blocked, None).map(|d| d.state),
            Some(AgentActivity::Blocked)
        );

        // Kimi welcome screen must NOT produce Working or Blocked
        let kimi_welcome = ScreenInput {
            rows: vec![
                "┌──────────────────────────────────────────────┐".to_string(),
                "│ Welcome to Kimi Code CLI!                    │".to_string(),
                "│ Send /help for help information.             │".to_string(),
                "│ Directory: /Users/example/code/project/orca-lite │".to_string(),
                "│ Session: 550e8400-e29b-41d4-a716-446655440000 │".to_string(),
                "│ Model: kimi-k1.5                             │".to_string(),
                "└──────────────────────────────────────────────┘".to_string(),
            ],
            title: "kimi".to_string(),
        };
        let kimi_welcome_det = engine.detect(&kimi_welcome, None);
        assert!(
            kimi_welcome_det.is_none()
                || kimi_welcome_det.as_ref().map(|d| d.state) == Some(AgentActivity::Idle),
            "kimi welcome screen must produce None or Idle, got: {kimi_welcome_det:?}"
        );
        assert_ne!(
            kimi_welcome_det.as_ref().map(|d| d.state),
            Some(AgentActivity::Working),
            "kimi welcome screen must not fabricate Working"
        );
        assert_ne!(
            kimi_welcome_det.as_ref().map(|d| d.state),
            Some(AgentActivity::Blocked),
            "kimi welcome screen must not fabricate Blocked"
        );
    }
}

// Captured-screen parity tests (2026-10-22). Every file under fixtures/screens/ is a
// verbatim copy of .omo-parity/facts/screens/<agent>/<name>.screen (tmux capture-pane -p).
// Manifests are evaluated globally, so each capture must be won by its own manifest, and
// the manifests added or extended for this parity pass must not fire on another agent.
#[cfg(test)]
mod parity_screens {
    use super::*;

    macro_rules! fixture {
        ($agent:literal, $name:literal) => {
            (
                concat!($agent, "/", $name),
                include_str!(concat!("fixtures/screens/", $agent, "/", $name, ".screen")),
            )
        };
    }

    type Fixture = (&'static str, &'static str);

    fn screen(text: &str, title: &str) -> ScreenInput {
        ScreenInput {
            rows: text.lines().map(str::to_string).collect(),
            title: title.to_string(),
        }
    }

    fn winner(text: &str, title: &str) -> Option<(String, AgentActivity)> {
        default_engine()
            .detect(&screen(text, title), None)
            .map(|d| (d.manifest_id, d.state))
    }

    fn assert_wins(manifest_id: &str, cases: &[(Fixture, AgentActivity)]) {
        for ((name, text), state) in cases {
            assert_eq!(
                winner(text, ""),
                Some((manifest_id.to_string(), *state)),
                "{name} must be detected as ({manifest_id}, {state:?})"
            );
        }
    }

    // (owning manifest id, fixture) for every captured screen in fixtures/screens/.
    fn all_fixtures() -> Vec<(&'static str, Fixture)> {
        vec![
            ("cline", fixture!("cline", "idle")),
            ("cline", fixture!("cline", "working")),
            ("cline", fixture!("cline", "approval")),
            ("aider", fixture!("aider", "approval-gitignore-confirm")),
            ("aider", fixture!("aider", "working-commit-message")),
            ("aider", fixture!("aider", "approval-run-shell")),
            ("aider", fixture!("aider", "idle-after-turn")),
            ("aider", fixture!("aider", "idle")),
            ("aider", fixture!("aider", "working-waiting-spinner")),
            ("aider", fixture!("aider", "approval-add-file")),
            ("aider", fixture!("aider", "login-wall")),
            ("copilot", fixture!("copilot", "idle")),
            ("copilot", fixture!("copilot", "working")),
            ("copilot", fixture!("copilot", "approval")),
            ("claude", fixture!("claude", "done")),
            ("claude", fixture!("claude", "idle")),
            ("claude", fixture!("claude", "working")),
            ("claude", fixture!("claude", "approval")),
            ("codex", fixture!("codex", "done")),
            ("codex", fixture!("codex", "idle")),
            ("codex", fixture!("codex", "working")),
            ("codex", fixture!("codex", "approval")),
            ("gjc", fixture!("gjc", "after-answer")),
            ("gjc", fixture!("gjc", "idle-after-turn")),
            ("gjc", fixture!("gjc", "idle")),
            ("gjc", fixture!("gjc", "working")),
            ("gjc", fixture!("gjc", "approval-ask")),
            ("grok", fixture!("grok", "idle-after-turn")),
            ("grok", fixture!("grok", "idle")),
            ("grok", fixture!("grok", "resumed")),
            ("grok", fixture!("grok", "working")),
            ("grok", fixture!("grok", "login-wall")),
            ("grok", fixture!("grok", "trust-folder")),
            ("grok", fixture!("grok", "after-approve")),
            ("grok", fixture!("grok", "approval")),
            ("omp", fixture!("omp", "idle-no-model")),
            ("omp", fixture!("omp", "idle-after-interrupt")),
            ("omp", fixture!("omp", "idle-after-turn")),
            ("omp", fixture!("omp", "idle")),
            ("omp", fixture!("omp", "question-single")),
            ("omp", fixture!("omp", "working")),
            ("omp", fixture!("omp", "working-2")),
            ("omp", fixture!("omp", "idle-after-cancel-retry")),
            ("omp", fixture!("omp", "question-multi")),
            ("omp", fixture!("omp", "idle-resumed")),
            ("omp", fixture!("omp", "approval")),
            ("pi", fixture!("pi", "done")),
            ("pi", fixture!("pi", "idle")),
            ("pi", fixture!("pi", "resumed")),
            ("pi", fixture!("pi", "working")),
            ("pi", fixture!("pi", "local1-idle")),
            ("pi", fixture!("pi", "approval")),
            ("pi", fixture!("pi", "lc-idle-no-model")),
            ("antigravity", fixture!("agy", "lc-working")),
            ("antigravity", fixture!("agy", "idle")),
            ("antigravity", fixture!("agy", "working")),
            ("antigravity", fixture!("agy", "lc-idle")),
            ("antigravity", fixture!("agy", "resume")),
            ("antigravity", fixture!("agy", "trust")),
            ("droid", fixture!("droid", "login-wall")),
            ("droid", fixture!("droid", "login-device-auth")),
            ("crush", fixture!("crush", "after-allow")),
            ("crush", fixture!("crush", "idle-after-turn")),
            ("crush", fixture!("crush", "idle")),
            ("crush", fixture!("crush", "working")),
            ("crush", fixture!("crush", "resumed-session")),
            ("crush", fixture!("crush", "approval")),
            ("cursor", fixture!("cursor-agent", "login-required")),
            ("cursor", fixture!("cursor-agent", "live-trust")),
            ("cursor", fixture!("cursor-agent", "live-working")),
            ("cursor", fixture!("cursor-agent", "live-running")),
            ("cursor", fixture!("cursor-agent", "live-approval")),
            ("cursor", fixture!("cursor-agent", "live-idle")),
            ("cursor", fixture!("cursor-agent", "live2-trust")),
            ("cursor", fixture!("cursor-agent", "live2-approval")),
            ("kimi", fixture!("kimi", "idle")),
            ("kimi", fixture!("kimi", "working")),
            ("kimi", fixture!("kimi", "resume")),
            ("opencode", fixture!("opencode", "idle")),
            ("opencode", fixture!("opencode", "working")),
            ("opencode", fixture!("opencode", "approval")),
            ("claude", fixture!("claude", "local1-idle")),
            ("claude", fixture!("claude", "after-turn-idle")),
            ("claude", fixture!("claude", "resumed")),
            ("claude", fixture!("claude", "local1-working")),
            ("claude", fixture!("claude", "lc-working")),
            ("codex", fixture!("codex", "after-turn-idle")),
            ("codex", fixture!("codex", "resumed")),
            ("codex", fixture!("codex", "local1-working")),
            ("codex", fixture!("codex", "lc-working")),
            ("opencode", fixture!("opencode", "done")),
            ("opencode", fixture!("opencode", "resumed")),
            ("opencode", fixture!("opencode", "local1-working")),
            ("opencode", fixture!("opencode", "lc-working")),
            ("opencode", fixture!("opencode", "lc-approval")),
            ("copilot", fixture!("copilot", "local3-idle")),
            ("copilot", fixture!("copilot", "after-turn")),
            ("copilot", fixture!("copilot", "resumed")),
            ("copilot", fixture!("copilot", "local3-working")),
            ("copilot", fixture!("copilot", "working-modelfail")),
            ("cline", fixture!("cline", "after-turn-idle")),
            ("cline", fixture!("cline", "idle-after-turn")),
            ("cline", fixture!("cline", "resumed")),
            ("cline", fixture!("cline", "lc-working")),
        ]
    }

    #[test]
    fn omp_captured_screens_reach_expected_state() {
        use AgentActivity::*;
        assert_wins(
            "omp",
            &[
                (fixture!("omp", "idle"), Idle),
                (fixture!("omp", "idle-after-turn"), Idle),
                (fixture!("omp", "idle-after-interrupt"), Idle),
                (fixture!("omp", "idle-resumed"), Idle),
                (fixture!("omp", "idle-no-model"), Idle),
                (fixture!("omp", "idle-after-cancel-retry"), Idle),
                (fixture!("omp", "working"), Working),
                (fixture!("omp", "working-2"), Working),
                (fixture!("omp", "approval"), Blocked),
                (fixture!("omp", "question-single"), Blocked),
                (fixture!("omp", "question-multi"), Blocked),
            ],
        );
    }

    #[test]
    fn pi_captured_screens_reach_expected_state() {
        use AgentActivity::*;
        assert_wins(
            "pi",
            &[
                (fixture!("pi", "idle"), Idle),
                (fixture!("pi", "local1-idle"), Idle),
                (fixture!("pi", "done"), Idle),
                (fixture!("pi", "resumed"), Idle),
                (fixture!("pi", "lc-idle-no-model"), Idle),
                (fixture!("pi", "working"), Working),
                (fixture!("pi", "approval"), Blocked),
            ],
        );
    }

    // pi and omo draw the same senpi chrome; only omo adds the "(😺 OmO Native by Q Kim)"
    // tag line (.omo-parity/facts/local-1.md). The same pi footer with that line is omo's.
    #[test]
    fn pi_footer_with_the_omo_tag_line_stays_omo() {
        let (_, text) = fixture!("pi", "idle");
        let text = format!("{text}\n (😺 OmO Native by Q Kim)");
        assert_eq!(
            winner(&text, ""),
            Some(("omo".to_string(), AgentActivity::Idle))
        );
    }

    #[test]
    fn crush_captured_screens_reach_expected_state() {
        use AgentActivity::*;
        assert_wins(
            "crush",
            &[
                (fixture!("crush", "idle"), Idle),
                (fixture!("crush", "idle-after-turn"), Idle),
                (fixture!("crush", "after-allow"), Idle),
                (fixture!("crush", "resumed-session"), Idle),
                (fixture!("crush", "working"), Working),
                (fixture!("crush", "approval"), Blocked),
            ],
        );
    }

    #[test]
    fn aider_captured_screens_reach_expected_state() {
        use AgentActivity::*;
        assert_wins(
            "aider",
            &[
                (fixture!("aider", "idle"), Idle),
                (fixture!("aider", "idle-after-turn"), Idle),
                (fixture!("aider", "working-waiting-spinner"), Working),
                (fixture!("aider", "working-commit-message"), Working),
                (fixture!("aider", "approval-gitignore-confirm"), Blocked),
                (fixture!("aider", "approval-add-file"), Blocked),
                (fixture!("aider", "approval-run-shell"), Blocked),
                (fixture!("aider", "login-wall"), Blocked),
            ],
        );
    }

    #[test]
    fn grok_captured_screens_reach_expected_state() {
        use AgentActivity::*;
        assert_wins(
            "grok",
            &[
                (fixture!("grok", "idle"), Idle),
                (fixture!("grok", "idle-after-turn"), Idle),
                (fixture!("grok", "after-approve"), Idle),
                (fixture!("grok", "resumed"), Idle),
                (fixture!("grok", "working"), Working),
                (fixture!("grok", "approval"), Blocked),
                (fixture!("grok", "trust-folder"), Blocked),
                (fixture!("grok", "login-wall"), Blocked),
            ],
        );
    }

    #[test]
    fn gjc_captured_screens_reach_expected_state() {
        use AgentActivity::*;
        assert_wins(
            "gjc",
            &[
                (fixture!("gjc", "idle"), Idle),
                (fixture!("gjc", "idle-after-turn"), Idle),
                (fixture!("gjc", "after-answer"), Idle),
                (fixture!("gjc", "working"), Working),
                (fixture!("gjc", "approval-ask"), Blocked),
            ],
        );
    }

    #[test]
    fn antigravity_captured_screens_reach_expected_state() {
        use AgentActivity::*;
        assert_wins(
            "antigravity",
            &[
                (fixture!("agy", "idle"), Idle),
                (fixture!("agy", "resume"), Idle),
                (fixture!("agy", "lc-idle"), Idle),
                (fixture!("agy", "working"), Working),
                (fixture!("agy", "lc-working"), Working),
                (fixture!("agy", "trust"), Blocked),
            ],
        );
    }

    // Only the login wall and device-auth screens of droid 0.247.0 could be captured
    // (.omo-parity/facts/remote-droid.md).
    #[test]
    fn droid_captured_screens_reach_expected_state() {
        use AgentActivity::*;
        assert_wins(
            "droid",
            &[
                (fixture!("droid", "login-wall"), Blocked),
                (fixture!("droid", "login-device-auth"), Blocked),
            ],
        );
    }

    #[test]
    fn kimi_captured_screens_reach_expected_state() {
        use AgentActivity::*;
        assert_wins(
            "kimi",
            &[
                (fixture!("kimi", "idle"), Idle),
                (fixture!("kimi", "resume"), Idle),
                (fixture!("kimi", "working"), Working),
            ],
        );
    }

    // claude 2.1.296/2.1.308: the mode footer gains "esc to interrupt" while a turn runs. In
    // working.screen the next prompt is typed but not yet sent, so it is idle.
    #[test]
    fn claude_captured_screens_reach_expected_state() {
        use AgentActivity::*;
        assert_wins(
            "claude",
            &[
                (fixture!("claude", "idle"), Idle),
                (fixture!("claude", "local1-idle"), Idle),
                (fixture!("claude", "after-turn-idle"), Idle),
                (fixture!("claude", "done"), Idle),
                (fixture!("claude", "resumed"), Idle),
                (fixture!("claude", "working"), Idle),
                (fixture!("claude", "local1-working"), Working),
                (fixture!("claude", "lc-working"), Working),
                (fixture!("claude", "approval"), Blocked),
            ],
        );
    }

    // codex 0.156.1/0.160.0: "• Working (… esc to interrupt)" above the "› Ask Codex" composer.
    #[test]
    fn codex_captured_screens_reach_expected_state() {
        use AgentActivity::*;
        assert_wins(
            "codex",
            &[
                (fixture!("codex", "idle"), Idle),
                (fixture!("codex", "after-turn-idle"), Idle),
                (fixture!("codex", "done"), Idle),
                (fixture!("codex", "resumed"), Idle),
                (fixture!("codex", "working"), Working),
                (fixture!("codex", "local1-working"), Working),
                (fixture!("codex", "lc-working"), Working),
                (fixture!("codex", "approval"), Blocked),
            ],
        );
    }

    // opencode 1.18.18: the progress bar row while working, the "△ Permission required" card.
    #[test]
    fn opencode_captured_screens_reach_expected_state() {
        use AgentActivity::*;
        assert_wins(
            "opencode",
            &[
                (fixture!("opencode", "idle"), Idle),
                (fixture!("opencode", "done"), Idle),
                (fixture!("opencode", "resumed"), Idle),
                (fixture!("opencode", "working"), Working),
                (fixture!("opencode", "local1-working"), Working),
                (fixture!("opencode", "lc-working"), Working),
                (fixture!("opencode", "approval"), Blocked),
                (fixture!("opencode", "lc-approval"), Blocked),
            ],
        );
    }

    // Copilot CLI 0.0.375: "<glyph> Thinking (Esc to cancel)" while working, composer otherwise.
    #[test]
    fn copilot_captured_screens_reach_expected_state() {
        use AgentActivity::*;
        assert_wins(
            "copilot",
            &[
                (fixture!("copilot", "idle"), Idle),
                (fixture!("copilot", "local3-idle"), Idle),
                (fixture!("copilot", "after-turn"), Idle),
                (fixture!("copilot", "resumed"), Idle),
                (fixture!("copilot", "working"), Working),
                (fixture!("copilot", "local3-working"), Working),
                (fixture!("copilot", "working-modelfail"), Working),
                (fixture!("copilot", "approval"), Blocked),
            ],
        );
    }

    // cline: " ⠏ Thinking... (esc to cancel)" while working; it used to be won by cursor.
    #[test]
    fn cline_captured_screens_reach_expected_state() {
        use AgentActivity::*;
        assert_wins(
            "cline",
            &[
                (fixture!("cline", "idle"), Idle),
                (fixture!("cline", "after-turn-idle"), Idle),
                (fixture!("cline", "idle-after-turn"), Idle),
                (fixture!("cline", "resumed"), Idle),
                (fixture!("cline", "working"), Working),
                (fixture!("cline", "lc-working"), Working),
                (fixture!("cline", "approval"), Blocked),
            ],
        );
    }

    // cursor-agent 2026.10.19 logged in (.omo-parity/facts/cursor-live-screens.md): the
    // "→ Add a follow-up" composer ends in "ctrl+c to stop" while a turn runs and not after
    // it; "Run this command?" is the shell approval card. live-trust is the login wall
    // ("Press any key to log in..."), the same screen as login-required.
    #[test]
    fn cursor_captured_screens_reach_expected_state() {
        use AgentActivity::*;
        assert_wins(
            "cursor",
            &[
                (fixture!("cursor-agent", "live-idle"), Idle),
                (fixture!("cursor-agent", "live-working"), Working),
                (fixture!("cursor-agent", "live-running"), Working),
                (fixture!("cursor-agent", "live-approval"), Blocked),
                (fixture!("cursor-agent", "live-trust"), Blocked),
                (fixture!("cursor-agent", "login-required"), Blocked),
                (fixture!("cursor-agent", "live2-trust"), Blocked),
                (fixture!("cursor-agent", "live2-approval"), Blocked),
            ],
        );
    }

    // The cursor trust box as drawn before [a] is pressed, captured live (live2-trust,
    // cursor-hooks-live-2026-10-23.md). live2-approval is the same run after [a]: the box
    // stays in scrollback with "⏳ Trusting workspace...", so the shell card must win it.
    #[test]
    fn cursor_trust_box_blocks_until_trust_is_given() {
        for ((name, text), rule) in [
            (fixture!("cursor-agent", "live2-trust"), "trust_folder_blocked"),
            (fixture!("cursor-agent", "live2-approval"), "run_command_card_blocked"),
        ] {
            assert_eq!(
                default_engine()
                    .detect(&screen(text, ""), None)
                    .map(|d| (d.manifest_id, d.state, d.rule_id)),
                Some(("cursor".to_string(), AgentActivity::Blocked, rule.to_string())),
                "{name}"
            );
        }
    }

    // Other agents' captures must never be won by a manifest from this parity pass.
    #[test]
    fn other_agents_captures_are_not_won_by_parity_manifests() {
        const PARITY: [&str; 15] = [
            "claude",
            "codex",
            "opencode",
            "copilot",
            "cline",
            "cursor",
            "omp",
            "pi",
            "crush",
            "aider",
            "droid",
            "grok",
            "gjc",
            "antigravity",
            "kimi",
        ];
        for (owner, (name, text)) in all_fixtures() {
            if let Some((manifest_id, state)) = winner(text, "") {
                assert!(
                    manifest_id == owner || !PARITY.contains(&manifest_id.as_str()),
                    "{name} (owned by {owner}) was won by {manifest_id} as {state:?}"
                );
            }
        }
    }

    // Each new manifest, and each rule added to an existing manifest, evaluated on its own
    // against every capture of another agent: none may match at all.
    #[test]
    fn parity_rules_do_not_match_other_agents_captures() {
        let added: [(&str, Option<&[&str]>); 15] = [
            ("claude", Some(&["mode_footer_working", "mode_footer_idle"])),
            ("codex", Some(&["composer_working"])),
            (
                "cursor",
                Some(&[
                    "login_wall_blocked",
                    "trust_folder_blocked",
                    "run_command_card_blocked",
                    "composer_working",
                    "composer_idle",
                ]),
            ),
            (
                "opencode",
                Some(&["permission_buttons_blocked", "progress_bar_working"]),
            ),
            ("copilot", Some(&["thinking_row_working", "composer_idle"])),
            (
                "cline",
                Some(&[
                    "permission_card_blocked",
                    "thinking_row_working",
                    "composer_idle",
                ]),
            ),
            ("omp", None),
            ("pi", None),
            ("crush", None),
            ("aider", None),
            ("droid", None),
            (
                "grok",
                Some(&[
                    "device_auth_blocked",
                    "trust_folder_blocked",
                    "permission_hints_blocked",
                    "esc_cancel_hints_working",
                    "prompt_hints_idle",
                    "welcome_menu_idle",
                ]),
            ),
            (
                "gjc",
                Some(&["ask_selector_blocked", "esc_hint_working", "composer_idle"]),
            ),
            (
                "antigravity",
                Some(&[
                    "spinner_working",
                    "composer_footer_working",
                    "composer_footer_idle",
                    "trust_folder_blocked",
                ]),
            ),
            ("kimi", Some(&["status_bar_idle"])),
        ];
        for raw in SHIPPED_MANIFESTS {
            let mut manifest = load_manifest_from_str(raw).expect("shipped manifest");
            let Some((id, rule_ids)) = added.iter().find(|(id, _)| *id == manifest.id) else {
                continue;
            };
            if let Some(rule_ids) = rule_ids {
                manifest
                    .rules
                    .retain(|rule| rule_ids.contains(&rule.id.as_str()));
                assert_eq!(
                    manifest.rules.len(),
                    rule_ids.len(),
                    "{id}: rule ids drifted"
                );
            }
            let engine = DetectionEngine::new(vec![manifest]);
            for (owner, (name, text)) in all_fixtures() {
                if owner == *id {
                    continue;
                }
                assert_eq!(
                    engine.detect(&screen(text, ""), None),
                    None,
                    "{id} rule matched {name}"
                );
            }
        }
    }

    // Grok Build 0.2.x footers (herdr evidence) must keep working next to the 1.0.67 ones.
    #[test]
    fn grok_footer_rules_accept_old_and_new_hint_strings() {
        use AgentActivity::*;
        let cases = [
            ("1/3:select │ Ctrl+o:yolo │ Ctrl+c:cancel", Blocked),
            (
                "  1/4:select  │  Tab:next option  │  ←/→:scope  │  Ctrl+o:always-approve  │  Ctrl+c:cancel  │  Esc:scrollback",
                Blocked,
            ),
            ("Esc:cancel │ Ctrl+.:shortcuts", Working),
            ("  Shift+Tab:mode  │  Ctrl+c:cancel  │  Ctrl+x:shortcuts", Working),
            ("Ctrl+.:shortcuts", Idle),
            ("  Shift+Tab:mode  │  Ctrl+x:shortcuts", Idle),
        ];
        for (footer, state) in cases {
            assert_eq!(
                winner(footer, ""),
                Some(("grok".to_string(), state)),
                "{footer}"
            );
        }
    }
}

#[cfg(test)]
#[path = "independent_probe.rs"]
mod independent_probe;
