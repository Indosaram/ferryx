//! Observe and act through `observe.js`, the page script both engines run (plan C4).
//!
//! A ref names one DOM node (a WeakMap identity inside the page), not a CSS selector that is
//! recomputed at act time, so a replaced node fails as stale instead of the action landing on
//! its replacement. The host keeps, per tab and document generation, each ref's fingerprint
//! (tag, role, name) from the observe that issued it; an act sends that fingerprint along and
//! the page refuses a node that no longer matches. Navigation clears the record with the
//! generation.
//!
//! This module only builds scripts and reads results; `ipc::browser` runs them.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::browser::model::{
    BrowserAutomationAction, BrowserAutomationElement, BrowserAutomationFrame, BrowserWaitCondition,
};
use crate::browser::BrowserError;

/// The page script; evaluated as `(<source>)(<op>)`.
pub const OBSERVE_SOURCE: &str = include_str!("observe.js");

/// Most elements one observe returns; the snapshot reports `truncated` beyond it.
pub const MAX_OBSERVED_ELEMENTS: u32 = 1000;

/// Window property holding the page's ref store. Random per app run, so page code cannot
/// pre-seed it before the first observe.
fn store_key() -> &'static str {
    static KEY: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        format!("__ferryxAgent{}", uuid::Uuid::new_v4().simple())
    });
    &KEY
}

/// Every op carries every field (null when unused): the page reads them as plain properties,
/// so a missing one would fall through to whatever the page put on `Object.prototype`.
fn script(mut op: Value) -> String {
    if let Value::Object(fields) = &mut op {
        fields.insert("key".into(), Value::from(store_key()));
        for name in ["isolated", "max", "ref", "expect", "action", "check"] {
            fields.entry(name).or_insert(Value::Null);
        }
    }
    format!("({OBSERVE_SOURCE})({op})")
}

/// The observe script. `isolated` is true where it runs in an isolated world (Chromium engine
/// tabs), so it can use its own globals instead of building a clean realm.
pub fn observe_script(isolated: bool) -> String {
    script(json!({
        "op": "observe",
        "isolated": isolated,
        "max": MAX_OBSERVED_ELEMENTS,
    }))
}

/// The page-side form of an action, or `None` for actions that need no element (keypress,
/// window scroll, typing into the focused element), which keep their existing scripts.
fn page_action(action: &BrowserAutomationAction) -> Option<(&str, Value)> {
    use BrowserAutomationAction as A;
    let (reference, body) = match action {
        A::Click { reference } => (reference, json!({ "type": "click" })),
        A::Dblclick { reference } => (reference, json!({ "type": "dblclick" })),
        A::Hover { reference } => (reference, json!({ "type": "hover" })),
        A::Focus { reference } => (reference, json!({ "type": "focus" })),
        A::Check { reference } => (reference, json!({ "type": "check" })),
        A::Uncheck { reference } => (reference, json!({ "type": "uncheck" })),
        A::ScrollIntoView { reference } => (reference, json!({ "type": "scrollIntoView" })),
        A::Fill { reference, value } => (reference, json!({ "type": "fill", "value": value })),
        A::Select { reference, value } => (reference, json!({ "type": "select", "value": value })),
        A::Scroll { reference: Some(reference), x, y } => {
            (reference, json!({ "type": "scroll", "x": x.unwrap_or(0), "y": y.unwrap_or(0) }))
        }
        A::Type { reference: Some(reference), text } => (reference, json!({ "type": "type", "text": text })),
        A::Keypress { .. } | A::Scroll { reference: None, .. } | A::Type { reference: None, .. } => return None,
    };
    Some((reference.as_str(), body))
}

/// The act script for an element action, bound to the fingerprint the issuing observe recorded.
pub fn act_script(action: &BrowserAutomationAction, expect: &str, isolated: bool) -> Option<String> {
    let (reference, body) = page_action(action)?;
    Some(script(json!({
        "op": "act",
        "isolated": isolated,
        "ref": reference,
        "expect": expect,
        "action": body,
    })))
}

/// The script that resolves a ref and returns the viewport point a trusted click lands on
/// (`null` when the element has no box or something else covers it).
pub fn point_script(reference: &str, expect: &str, isolated: bool) -> String {
    script(json!({
        "op": "act",
        "isolated": isolated,
        "ref": reference,
        "expect": expect,
        "action": { "type": "point" },
    }))
}

/// The ref an action addresses, if any.
pub fn action_reference(action: &BrowserAutomationAction) -> Option<&str> {
    page_action(action).map(|(reference, _)| reference)
}

/// Page check for a postcondition, plus its own timeout when it carries one. Script
/// conditions are refused: an act postcondition never runs caller-supplied code.
pub fn postcondition_script(
    condition: &BrowserWaitCondition,
    isolated: bool,
) -> Result<(String, Option<u64>), BrowserError> {
    let (condition, timeout_ms) = match condition {
        BrowserWaitCondition::WithTimeout { inner, timeout_ms } => (inner.as_ref(), Some(*timeout_ms)),
        other => (other, None),
    };
    let (kind, value) = match condition {
        BrowserWaitCondition::Text { text } => ("text", text.as_str()),
        BrowserWaitCondition::Selector { selector } => ("selector", selector.as_str()),
        BrowserWaitCondition::UrlContains { fragment } => ("url", fragment.as_str()),
        BrowserWaitCondition::LoadState { state } => match state.to_ascii_lowercase().as_str() {
            "load" | "complete" => ("load", "complete"),
            "domcontentloaded" | "interactive" => ("load", "interactive"),
            _ => {
                return Err(BrowserError::AutomationFailed(format!(
                    "unsupported postcondition load state {state:?}: use load or domcontentloaded (network idle is not supported)"
                )))
            }
        },
        BrowserWaitCondition::Function { .. } => {
            return Err(BrowserError::AutomationFailed(
                "an act postcondition cannot run a script; use text, selector or url".into(),
            ))
        }
        BrowserWaitCondition::WithTimeout { .. } => {
            return Err(BrowserError::AutomationFailed("a postcondition can carry one timeout".into()))
        }
    };
    let source = script(json!({
        "op": "check",
        "isolated": isolated,
        "check": { "kind": kind, "value": value },
    }));
    Ok((source, timeout_ms))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    ok: bool,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

/// One observed element as the page reports it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedElement {
    pub reference: String,
    pub role: String,
    pub name: String,
    pub tag_name: String,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub checked: Option<bool>,
}

impl ObservedElement {
    /// What the page compares before acting; mirrors `fingerprint` in observe.js (a text box's
    /// own content may name it, so its name is left out).
    pub fn fingerprint(&self) -> String {
        let name = if matches!(self.role.as_str(), "textbox" | "searchbox") { "" } else { &self.name };
        format!("{}\n{}\n{}", self.tag_name, self.role, name)
    }

    pub fn into_element(self) -> BrowserAutomationElement {
        BrowserAutomationElement {
            reference: self.reference,
            role: self.role,
            name: self.name,
            tag_name: self.tag_name,
            disabled: self.disabled,
            checked: self.checked,
        }
    }
}

/// A parsed observe.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub url: String,
    pub title: String,
    pub elements: Vec<ObservedElement>,
    #[serde(default)]
    pub frames: Vec<BrowserAutomationFrame>,
    #[serde(default)]
    pub truncated: bool,
}

/// An evaluation result is the JSON text of the script's value; a value that is itself a
/// JSON string (a page that returned text) is unwrapped once.
fn value_of(raw: &str) -> Result<Value, BrowserError> {
    let value: Value = serde_json::from_str(raw.trim())
        .map_err(|error| BrowserError::AutomationFailed(format!("invalid page result: {error}")))?;
    match value {
        Value::String(text) => serde_json::from_str(&text)
            .map_err(|error| BrowserError::AutomationFailed(format!("invalid page result: {error}"))),
        other => Ok(other),
    }
}

fn checked(value: Value) -> Result<Value, BrowserError> {
    let envelope: Envelope = serde_json::from_value(value.clone())
        .map_err(|error| BrowserError::AutomationFailed(format!("invalid page result: {error}")))?;
    if envelope.ok {
        return Ok(value);
    }
    let message = envelope.message.unwrap_or_else(|| "the page refused the action".into());
    match envelope.code.as_deref() {
        // A ref from another document, a removed or replaced node, or a node that changed
        // identity: the caller must observe again.
        Some("stale" | "changed") => Err(BrowserError::AutomationSnapshotStale),
        Some("unknown") => Err(BrowserError::AutomationTargetNotFound(message)),
        _ => Err(BrowserError::AutomationFailed(message)),
    }
}

pub fn parse_observation(raw: &str) -> Result<Observation, BrowserError> {
    serde_json::from_value(checked(value_of(raw)?)?)
        .map_err(|error| BrowserError::AutomationFailed(format!("invalid observe result: {error}")))
}

/// Reads an act result: `Ok(())` once the page dispatched it.
pub fn parse_act(raw: &str) -> Result<(), BrowserError> {
    checked(value_of(raw)?).map(|_| ())
}

/// Reads a point result: the viewport point, or `None` when a trusted click cannot land.
pub fn parse_point(raw: &str) -> Result<Option<(f64, f64)>, BrowserError> {
    let value = checked(value_of(raw)?)?;
    let point = value.get("point").filter(|point| !point.is_null());
    Ok(point.and_then(|point| Some((point.get("x")?.as_f64()?, point.get("y")?.as_f64()?))))
}

/// Reads a postcondition check: whether the page matched it.
pub fn parse_check(raw: &str) -> Result<bool, BrowserError> {
    let value = checked(value_of(raw)?)?;
    Ok(value.get("matched").and_then(Value::as_bool).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(reference: &str, role: &str, name: &str, tag: &str) -> ObservedElement {
        ObservedElement {
            reference: reference.into(),
            role: role.into(),
            name: name.into(),
            tag_name: tag.into(),
            disabled: false,
            checked: None,
        }
    }

    #[test]
    fn scripts_carry_the_op_and_never_the_page_text() {
        let observe = observe_script(false);
        assert!(observe.starts_with('(') && observe.ends_with(')'));
        assert!(observe.contains(r#""op":"observe""#));
        assert!(observe.contains(r#""max":1000"#));
        let fill = act_script(
            &BrowserAutomationAction::Fill { reference: "d1.4".into(), value: "x\"); alert(1); //".into() },
            "input\ntextbox\n",
            true,
        )
        .expect("element action");
        assert!(fill.contains(r#""ref":"d1.4""#));
        assert!(fill.contains(r#""value":"x\"); alert(1); //""#));
        assert!(fill.contains(r#""isolated":true"#));
        assert!(act_script(&BrowserAutomationAction::Keypress { key: "Enter".into() }, "", false).is_none());
        assert_eq!(
            action_reference(&BrowserAutomationAction::Type { reference: Some("d1.2".into()), text: "a".into() }),
            Some("d1.2")
        );
        assert_eq!(action_reference(&BrowserAutomationAction::Scroll { reference: None, x: None, y: Some(9) }), None);
    }

    #[test]
    fn observations_parse_from_both_engines_result_shapes() {
        let page = r#"{"ok":true,"url":"https://a.test/","title":"A","doc":"d1","elements":[{"reference":"d1.1","role":"button","name":"Save","tagName":"button","disabled":false,"checked":null}],"frames":[{"kind":"iframe","title":"Other","src":"https://b.test/frame"}],"truncated":true}"#;
        let direct = parse_observation(page).expect("object result");
        let wrapped = parse_observation(&serde_json::to_string(page).unwrap()).expect("string result");
        assert_eq!(direct, wrapped);
        assert!(direct.truncated);
        assert_eq!(direct.frames[0].src, "https://b.test/frame");
        assert_eq!(direct.elements[0].fingerprint(), "button\nbutton\nSave");
    }

    #[test]
    fn a_replaced_or_foreign_ref_is_stale_and_an_unknown_one_not_found() {
        let stale = r#"{"ok":false,"code":"stale","message":"the element was removed or replaced; observe again"}"#;
        assert_eq!(parse_act(stale), Err(BrowserError::AutomationSnapshotStale));
        let changed = r#"{"ok":false,"code":"changed","message":"x"}"#;
        assert_eq!(parse_act(changed), Err(BrowserError::AutomationSnapshotStale));
        let unknown = r#"{"ok":false,"code":"unknown","message":"no element has this ref"}"#;
        assert!(matches!(parse_act(unknown), Err(BrowserError::AutomationTargetNotFound(_))));
        let refused = r#"{"ok":false,"code":"noOption","message":"no option has this value or text"}"#;
        assert!(matches!(parse_act(refused), Err(BrowserError::AutomationFailed(_))));
        assert_eq!(parse_act(r#"{"ok":true}"#), Ok(()));
        assert!(parse_act("undefined").is_err());
    }

    #[test]
    fn a_text_box_is_matched_without_its_name() {
        assert_eq!(element("d.1", "textbox", "typed text", "div").fingerprint(), "div\ntextbox\n");
        assert_eq!(element("d.2", "link", "Docs", "a").fingerprint(), "a\nlink\nDocs");
    }

    #[test]
    fn points_and_checks_read_their_values() {
        assert_eq!(parse_point(r#"{"ok":true,"point":{"x":4.5,"y":8}}"#), Ok(Some((4.5, 8.0))));
        assert_eq!(parse_point(r#"{"ok":true,"point":null}"#), Ok(None));
        assert_eq!(parse_check(r#"{"ok":true,"matched":true}"#), Ok(true));
        assert_eq!(parse_check(r#"{"ok":true,"matched":false}"#), Ok(false));
    }

    #[test]
    fn postconditions_take_page_state_only() {
        let (text, timeout) = postcondition_script(
            &BrowserWaitCondition::WithTimeout {
                inner: Box::new(BrowserWaitCondition::Text { text: "Saved".into() }),
                timeout_ms: 2000,
            },
            false,
        )
        .expect("text");
        assert!(text.contains(r#""kind":"text","value":"Saved""#));
        assert_eq!(timeout, Some(2000));
        let (load, _) =
            postcondition_script(&BrowserWaitCondition::LoadState { state: "DOMContentLoaded".into() }, true).unwrap();
        assert!(load.contains(r#""value":"interactive""#));
        assert!(postcondition_script(&BrowserWaitCondition::Function { script: "true".into() }, false).is_err());
        assert!(postcondition_script(&BrowserWaitCondition::LoadState { state: "networkidle".into() }, false).is_err());
    }
}
