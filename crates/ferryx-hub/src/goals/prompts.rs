//! Prompt text of the goal evaluators. The gap assessment sees the contract, the evidence
//! (each ref labelled attested or unattested) and the recent turn ledger, and must answer
//! with one strict JSON [`Assessment`](super::types::Assessment).
use super::contract::Contract;
use super::evidence::{is_attested_ref, EvidenceBundle};
use super::turns::Turn;
use super::types::{EvidenceSource, TurnOrigin};

/// Tag of the block telling the evaluator it owns completion against the contract.
pub const OWNERSHIP_TAG: &str = "ferryx-review-ownership";

/// How many of the latest turns a prompt shows.
pub const RECENT_TURNS_MAX: usize = 12;

const OWNERSHIP_TEXT: &str = "The Ferryx supervisor owns completion of this lane, and you grade it. Verify the work against the goal contract below yourself. The worker agent's report and its claims of being done are unattested: never rubber-stamp them. Human corrections in the turn ledger are the owner's direction and outrank the worker and any earlier proposal.";

/// Gap assessment rules 1-7.
pub const GAP_ASSESS_RULES: &str = "Rules:
1. Assess the current state first, then the target state in the contract, then the gaps between them.
2. Use only attested evidence refs to judge a requirement met. An unattested ref (an agent claim) never shows a requirement met on its own.
3. A claim of missing access without an observed attempt is a work gap, not external_input.
4. Choices within the lane's scope are work gaps. Use external_input only for input only the owner can give.
5. Return exactly one decision. A continue carries ONE instruction of at most 4000 characters plus the gap_ids it closes; every gap id must be a gap you listed, and every gap's requirement_ids must be contract requirement ids.
6. A stop reason may only be no_further_progress or high_stakes_user_required. Turn limits, delivery and evidence checks are decided by Ferryx, never by you.
7. Output only strict JSON matching the schema below: no prose, no code fence.";

/// The output schema the gap assessment must match.
pub const GAP_ASSESS_SCHEMA: &str = r#"{"gaps":[{"id":"g1","kind":"work"|"verification"|"goal_refinement"|"external_input","requirement_ids":["r1"],"summary":"..."}],
 "decision": one of
   {"decision":"continue","instruction":"<one next step>","gap_ids":["g1"],"expected_change":"...","verification":"<attested ref or check>"}
   {"decision":"stop","reason":"no_further_progress"|"high_stakes_user_required"}
   {"decision":"complete"}}"#;

/// Untrusted text never opens or closes a prompt block.
fn fence(text: &str) -> String {
    text.replace('<', "‹").replace('>', "›")
}

fn one_line(text: &str) -> String {
    fence(&text.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// The ownership block every evaluator prompt opens with.
pub fn ownership_block() -> String {
    format!("<{OWNERSHIP_TAG}>\n{OWNERSHIP_TEXT}\n</{OWNERSHIP_TAG}>\n")
}

/// The contract's requirements with status and evidence refs.
pub fn contract_block(contract: &Contract) -> String {
    let mut block = format!(
        "<goal-contract lane=\"{}\" revision=\"{}\" turns_used=\"{}\" turn_limit=\"{}\">\n",
        one_line(&contract.lane_key),
        contract.revision,
        contract.turns_used,
        contract.turn_limit,
    );
    for requirement in &contract.requirements {
        let refs = if requirement.evidence.is_empty() {
            "none".to_owned()
        } else {
            one_line(&requirement.evidence.join(","))
        };
        block.push_str(&format!(
            "- id={} status={} evidence={}\n  requirement: {}\n  verifier: {}\n",
            one_line(&requirement.id),
            requirement.status,
            refs,
            one_line(&requirement.text),
            one_line(&requirement.verifier),
        ));
    }
    block.push_str("</goal-contract>\n");
    block
}

/// True when an evidence item is Ferryx-attested.
pub fn item_attested(source: EvidenceSource, reference: &str) -> bool {
    source != EvidenceSource::AgentClaim && is_attested_ref(reference)
}

/// Every evidence item, labelled `attested` or `unattested`.
pub fn evidence_block(evidence: &EvidenceBundle) -> String {
    let mut block = String::from("<evidence>\n");
    if evidence.items.is_empty() {
        block.push_str("(no evidence collected)\n");
    }
    for item in &evidence.items {
        let label = if item_attested(item.source, &item.reference) {
            "attested"
        } else {
            "unattested"
        };
        let passed = match item.passed {
            Some(true) => " passed=true",
            Some(false) => " passed=false",
            None => "",
        };
        block.push_str(&format!(
            "- ref={} {label} source={}{passed}\n  {}\n",
            one_line(&item.reference),
            item.source,
            fence(&crate::judge::scrub_for_external(&item.detail)).replace('\n', "\n  "),
        ));
    }
    block.push_str("</evidence>\n");
    block
}

/// The latest [`RECENT_TURNS_MAX`] turns, with owner corrections and raw typing called out.
pub fn turns_block(turns: &[Turn]) -> String {
    let mut block = String::from("<recent-turns>\n");
    if turns.is_empty() {
        block.push_str("(no turns recorded)\n");
    }
    for turn in &turns[turns.len().saturating_sub(RECENT_TURNS_MAX)..] {
        let text = match (turn.origin, &turn.text_excerpt) {
            (TurnOrigin::HumanCorrection, Some(text)) => {
                format!("OWNER CORRECTION: {}", one_line(text))
            }
            (TurnOrigin::HumanCorrection, None) => {
                "OWNER KEYSTROKES (typed into the pane; text not recorded)".to_owned()
            }
            (_, Some(text)) => one_line(text),
            (_, None) => "(no text)".to_owned(),
        };
        block.push_str(&format!("- #{} {}: {text}\n", turn.seq, turn.origin));
    }
    block.push_str("</recent-turns>\n");
    block
}

/// The full gap assessment prompt.
pub fn gap_assess_prompt(contract: &Contract, evidence: &EvidenceBundle, turns: &[Turn]) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{GAP_ASSESS_RULES}\n\nOutput schema:\n{GAP_ASSESS_SCHEMA}\n",
        ownership_block(),
        contract_block(contract),
        evidence_block(evidence),
        turns_block(turns),
    )
}

/// The retry prompt: the original prompt plus why the previous answer was refused.
pub fn retry_prompt(prompt: &str, error: &str) -> String {
    format!(
        "{prompt}\nYour previous answer was refused: {}\nAnswer again with only the strict JSON.\n",
        one_line(error)
    )
}
