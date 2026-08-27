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

    // 8. per-agent realistic screens: for omo, opencode, codex, claude, gemini —
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

        // --- GEMINI ---
        let gemini_working = ScreenInput {
            rows: vec![
                "Generating answer...".to_string(),
                "esc to cancel".to_string(),
            ],
            title: "gemini".to_string(),
        };
        assert_eq!(
            engine.detect(&gemini_working, None).map(|d| d.state),
            Some(AgentActivity::Working)
        );

        let gemini_blocked = ScreenInput {
            rows: vec![
                "Allow execution of write_file?".to_string(),
                "Apply this change".to_string(),
            ],
            title: "gemini".to_string(),
        };
        assert_eq!(
            engine.detect(&gemini_blocked, None).map(|d| d.state),
            Some(AgentActivity::Blocked)
        );

        let gemini_idle = ScreenInput {
            rows: vec!["Ready for queries.".to_string()],
            title: "◇ Ready (orca-lite)".to_string(),
        };
        assert_eq!(
            engine.detect(&gemini_idle, None).map(|d| d.state),
            Some(AgentActivity::Idle)
        );
    }
}

#[cfg(test)]
#[path = "independent_probe.rs"]
mod independent_probe;
