//! Verified replacement delta for one rev3 history read (plan T04, computation half).
//!
//! Pure: no file IO and no generation-tracker mutation. The route reads a page the same way a
//! rev2 read would, then calls [`reference_history_negotiate`]. For an `after` query that page
//! must be the newest page of the same bounded window. This module does not edit the route.
//!
//! The client's anchor is the wire `after` object: generation, the turn it will replace from
//! (its oldest open turn, or its newest closed turn when none is open), and the fingerprint of
//! the turn before that anchor. The open set is read from the page. An open turn before the
//! anchor is included, so the replacement is the open turns, everything after the anchor, and
//! any turns appended since.

use crate::scoped_contracts::{ScopeError, ScopeErrorCode};

use super::history::{
    reference_history_open_turn_indexes, reference_history_reader_may_align, reference_turn_fingerprint,
    reference_turn_is_open,
};
use super::types::{
    ReferenceHistoryAnchor, ReferenceHistoryCursor, ReferenceHistoryDeltaResponse, ReferenceHistoryPage,
    ReferenceHistoryResetReason, ReferenceTurn, REFERENCE_HISTORY_CURSOR_AND_AFTER,
};

/// A replacement longer than one max page is too far behind. The client takes the inline page.
pub const REFERENCE_HISTORY_DELTA_MAX_TURNS: usize = super::history::REFERENCE_HISTORY_MAX_LIMIT;

/// A replacement larger than the read window is too far behind. The bound is the turn list, not
/// the inline reset page (that page is already capped by the reader).
pub const REFERENCE_HISTORY_DELTA_MAX_BYTES: usize = super::history::REFERENCE_HISTORY_MAX_BYTES;

/// What the history route should write for one read.
#[derive(Debug, Clone, PartialEq)]
pub enum ReferenceHistoryRead<'page> {
    /// Rev2. This is the `page` argument, not a rebuilt copy.
    Page(&'page ReferenceHistoryPage),
    /// Rev3 replacement delta or inline reset.
    Negotiated(Box<ReferenceHistoryDeltaResponse>),
}

/// Negotiate a history read.
///
/// `cursor` is the request cursor, not the page's older-page cursor.
///
/// * `cursor` and `after` together is [`REFERENCE_HISTORY_CURSOR_AND_AFTER`]
///   ([`ScopeErrorCode::InvalidRequest`]).
/// * No `after` returns [`ReferenceHistoryRead::Page`] pointing at `page`. Serializing that
///   page is byte-identical to the rev2 response.
/// * A foreign `after.stream_id` is [`ScopeErrorCode::TargetExpired`]. Provider rotation stays
///   the route's existing target check; this function does not see the provider id.
/// * Otherwise the result is a replacement delta or a typed reset. Reset reasons, in check order:
///   `notNative` (availability is not native), `unsupportedProjection` (pi, cursor, opencode,
///   crush, cline, or unavailable), `unalignedTurns` (a turn has no id), `generationChanged`,
///   `anchorOutsideWindow` (the anchor id is absent and the page still has older history:
///   `has_more`, an older-page cursor, or `unavailable_reason`), `anchorNotTurnStart` (the id
///   is absent from a complete page), `anchorPrevMismatch`, then `tooFarBehind` (the suffix
///   exceeds [`REFERENCE_HISTORY_DELTA_MAX_TURNS`] or [`REFERENCE_HISTORY_DELTA_MAX_BYTES`]).
pub fn reference_history_negotiate<'page>(
    page: &'page ReferenceHistoryPage,
    stream_id: &str,
    cursor: Option<&ReferenceHistoryCursor>,
    after: Option<&ReferenceHistoryAnchor>,
) -> Result<ReferenceHistoryRead<'page>, ScopeError> {
    if cursor.is_some() && after.is_some() {
        return Err(refused(
            ScopeErrorCode::InvalidRequest,
            format!(
                "{REFERENCE_HISTORY_CURSOR_AND_AFTER}: cursor and after cannot be queried together"
            ),
        ));
    }
    let Some(after) = after else {
        return Ok(ReferenceHistoryRead::Page(page));
    };
    if after.stream_id != stream_id {
        return Err(refused(
            ScopeErrorCode::TargetExpired,
            format!(
                "the after anchor belongs to stream `{}`, not `{stream_id}`",
                after.stream_id
            ),
        ));
    }
    Ok(ReferenceHistoryRead::Negotiated(Box::new(delta_or_reset(page, after))))
}

fn refused(code: ScopeErrorCode, message: impl Into<String>) -> ScopeError {
    ScopeError {
        code,
        message: message.into(),
        retryable: false,
        details: serde_json::Value::Null,
    }
}

fn delta_or_reset(
    page: &ReferenceHistoryPage,
    after: &ReferenceHistoryAnchor,
) -> ReferenceHistoryDeltaResponse {
    if let Some(reason) = reset_reason(page, after) {
        return reset(page, reason);
    }
    let anchor_index = turn_index(page, &after.anchor_turn_id).expect("reset_reason accepted the anchor");
    let replace_from = replace_from_index(page, anchor_index);
    let turns = page.turns[replace_from..].to_vec();
    if turns.len() > REFERENCE_HISTORY_DELTA_MAX_TURNS || turns_exceed_bytes(&turns) {
        return reset(page, ReferenceHistoryResetReason::TooFarBehind);
    }
    let replace_from_turn_id = turns[0]
        .id
        .clone()
        .expect("an aligned turn has an id");
    ReferenceHistoryDeltaResponse::Delta {
        source: page.source,
        generation: page.generation.clone(),
        replace_from_turn_id,
        next_anchor: next_anchor(page, &after.stream_id),
        tail_open: page
            .turns
            .last()
            .is_some_and(|turn| reference_turn_is_open(turn, true)),
        turns,
    }
}

fn reset(
    page: &ReferenceHistoryPage,
    reason: ReferenceHistoryResetReason,
) -> ReferenceHistoryDeltaResponse {
    ReferenceHistoryDeltaResponse::Reset {
        reason,
        page: page.clone(),
    }
}

fn reset_reason(
    page: &ReferenceHistoryPage,
    after: &ReferenceHistoryAnchor,
) -> Option<ReferenceHistoryResetReason> {
    if !page.is_native() {
        return Some(ReferenceHistoryResetReason::NotNative);
    }
    if !reference_history_reader_may_align(page.source.native_kind()) {
        return Some(ReferenceHistoryResetReason::UnsupportedProjection);
    }
    if page.turns.iter().any(turn_lacks_id) {
        return Some(ReferenceHistoryResetReason::UnalignedTurns);
    }
    if after.generation != page.generation {
        return Some(ReferenceHistoryResetReason::GenerationChanged);
    }
    let Some(anchor_index) = turn_index(page, &after.anchor_turn_id) else {
        return Some(if anchor_outside_window(page) {
            ReferenceHistoryResetReason::AnchorOutsideWindow
        } else {
            ReferenceHistoryResetReason::AnchorNotTurnStart
        });
    };
    prefix_reason(page, anchor_index, after.previous_turn_fingerprint.as_deref())
}

fn turn_lacks_id(turn: &ReferenceTurn) -> bool {
    !matches!(turn.id.as_deref(), Some(id) if !id.is_empty())
}

/// The anchor fell out of this newest page: an older page remains, or the read cap disclosed
/// bytes it did not parse. A tail with fewer boundaries than the limit has `has_more == false`
/// and still names those unread bytes in `unavailable_reason`.
fn anchor_outside_window(page: &ReferenceHistoryPage) -> bool {
    page.has_more || page.cursor.is_some() || page.unavailable_reason.is_some()
}

fn turn_index(page: &ReferenceHistoryPage, id: &str) -> Option<usize> {
    if id.is_empty() {
        return None;
    }
    page.turns.iter().position(|turn| turn.id.as_deref() == Some(id))
}

fn prefix_reason(
    page: &ReferenceHistoryPage,
    anchor_index: usize,
    claimed: Option<&str>,
) -> Option<ReferenceHistoryResetReason> {
    if anchor_index == 0 {
        return claimed.map(|_| ReferenceHistoryResetReason::AnchorPrevMismatch);
    }
    match claimed {
        Some(claimed) if claimed == reference_turn_fingerprint(&page.turns[anchor_index - 1]) => None,
        _ => Some(ReferenceHistoryResetReason::AnchorPrevMismatch),
    }
}

/// Oldest open turn at or before the anchor, else the anchor. The suffix from there is the
/// open turns, everything after the anchor, and any appended turns.
fn replace_from_index(page: &ReferenceHistoryPage, anchor_index: usize) -> usize {
    reference_history_open_turn_indexes(&page.turns)
        .into_iter()
        .find(|index| *index <= anchor_index)
        .unwrap_or(anchor_index)
}

fn next_anchor(page: &ReferenceHistoryPage, stream_id: &str) -> ReferenceHistoryAnchor {
    let open = reference_history_open_turn_indexes(&page.turns);
    let index = open.first().copied().unwrap_or(page.turns.len() - 1);
    ReferenceHistoryAnchor {
        stream_id: stream_id.to_string(),
        generation: page.generation.clone(),
        anchor_turn_id: page.turns[index]
            .id
            .clone()
            .expect("an aligned turn has an id"),
        previous_turn_fingerprint: (index > 0).then(|| reference_turn_fingerprint(&page.turns[index - 1])),
    }
}

fn turns_exceed_bytes(turns: &[ReferenceTurn]) -> bool {
    match serde_json::to_vec(turns) {
        Ok(bytes) => bytes.len() > REFERENCE_HISTORY_DELTA_MAX_BYTES,
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::{
        ReferenceHistoryAvailability, ReferenceHistorySource, ReferencePart, ReferenceToolStatus,
        ReferenceTurnRole,
    };

    const STREAM: &str = "stream-1";

    fn text_turn(id: Option<&str>, role: ReferenceTurnRole, text: &str) -> ReferenceTurn {
        ReferenceTurn {
            role,
            started_at: None,
            ended_at: None,
            source: None,
            parts: vec![ReferencePart::Text {
                text: text.into(),
                phase: None,
                id: None,
            }],
            abandoned: None,
            id: id.map(str::to_string),
            meta: None,
        }
    }

    fn tool_turn(id: &str, call_id: &str, output: &str, status: ReferenceToolStatus) -> ReferenceTurn {
        ReferenceTurn {
            role: ReferenceTurnRole::Assistant,
            started_at: None,
            ended_at: None,
            source: None,
            parts: vec![ReferencePart::Tool {
                name: "read".into(),
                summary: String::new(),
                input: String::new(),
                output: output.into(),
                error: None,
                skill: None,
                output_ref: None,
                output_size: None,
                images: Vec::new(),
                id: None,
                call_id: Some(call_id.into()),
                status: Some(status),
                output_payload: Box::new(None),
            }],
            abandoned: None,
            id: Some(id.into()),
            meta: None,
        }
    }

    fn native_page(turns: Vec<ReferenceTurn>) -> ReferenceHistoryPage {
        ReferenceHistoryPage {
            source: ReferenceHistorySource::ClaudeTranscript,
            availability: ReferenceHistoryAvailability::Native,
            turns,
            cursor: None,
            has_more: false,
            generation: "gen-1".into(),
            unavailable_reason: None,
            composer: None,
        }
    }

    fn anchor(page: &ReferenceHistoryPage, turn_id: &str) -> ReferenceHistoryAnchor {
        let index = page.turns.iter().position(|turn| turn.id.as_deref() == Some(turn_id));
        ReferenceHistoryAnchor {
            stream_id: STREAM.into(),
            generation: page.generation.clone(),
            anchor_turn_id: turn_id.into(),
            previous_turn_fingerprint: index.and_then(|index| {
                (index > 0).then(|| reference_turn_fingerprint(&page.turns[index - 1]))
            }),
        }
    }

    fn expect_delta(
        page: &ReferenceHistoryPage,
        after: &ReferenceHistoryAnchor,
    ) -> (String, Vec<ReferenceTurn>, ReferenceHistoryAnchor, bool) {
        match reference_history_negotiate(page, STREAM, None, Some(after)) {
            Ok(ReferenceHistoryRead::Negotiated(body)) => match *body {
                ReferenceHistoryDeltaResponse::Delta {
                    source,
                    generation,
                    replace_from_turn_id,
                    turns,
                    next_anchor,
                    tail_open,
                } => {
                    assert_eq!(source, page.source);
                    assert_eq!(generation, page.generation);
                    (replace_from_turn_id, turns, next_anchor, tail_open)
                }
                ReferenceHistoryDeltaResponse::Reset { reason, .. } => {
                    panic!("expected a delta, reset {reason:?}")
                }
            },
            Ok(ReferenceHistoryRead::Page(_)) => panic!("expected a delta, got a rev2 page"),
            Err(error) => panic!("expected a delta, refused {}", error.message),
        }
    }

    fn expect_reset(
        page: &ReferenceHistoryPage,
        after: &ReferenceHistoryAnchor,
    ) -> ReferenceHistoryResetReason {
        match reference_history_negotiate(page, STREAM, None, Some(after)) {
            Ok(ReferenceHistoryRead::Negotiated(body)) => match *body {
                ReferenceHistoryDeltaResponse::Reset {
                    reason,
                    page: reset_page,
                } => {
                    assert_eq!(reset_page, *page);
                    reason
                }
                ReferenceHistoryDeltaResponse::Delta { .. } => {
                    panic!("expected a reset, got a delta")
                }
            },
            Ok(ReferenceHistoryRead::Page(_)) => panic!("expected a reset, got a rev2 page"),
            Err(error) => panic!("expected a reset, refused {}", error.message),
        }
    }

    #[test]
    fn append_only_delta() {
        let page = native_page(vec![
            text_turn(Some("stable"), ReferenceTurnRole::User, "ask"),
            text_turn(Some("reply"), ReferenceTurnRole::Assistant, "one"),
            text_turn(Some("reply-2"), ReferenceTurnRole::Assistant, "two"),
        ]);
        let (replace_from, turns, next, tail_open) = expect_delta(&page, &anchor(&page, "stable"));
        assert_eq!(replace_from, "stable");
        assert_eq!(turns, page.turns);
        assert!(tail_open);
        assert_eq!(next.anchor_turn_id, "reply-2");
        assert_eq!(next.stream_id, STREAM);
        assert_eq!(next.generation, page.generation);
        assert_eq!(
            next.previous_turn_fingerprint.as_deref(),
            Some(reference_turn_fingerprint(&page.turns[1]).as_str())
        );
    }

    #[test]
    fn open_turn_replaced_in_place() {
        let page = native_page(vec![
            text_turn(Some("user"), ReferenceTurnRole::User, "ask"),
            text_turn(Some("open"), ReferenceTurnRole::Assistant, "answer so far, still going"),
        ]);
        let (replace_from, turns, next, tail_open) = expect_delta(&page, &anchor(&page, "open"));
        assert_eq!(replace_from, "open");
        assert_eq!(turns.len(), 1);
        assert_eq!(turns, page.turns[1..]);
        assert_eq!(turns[0].id.as_deref(), Some("open"));
        assert!(tail_open);
        assert_eq!(next.anchor_turn_id, "open");
        assert_eq!(
            next.previous_turn_fingerprint.as_deref(),
            Some(reference_turn_fingerprint(&page.turns[0]).as_str())
        );
    }

    #[test]
    fn earlier_unanswered_call_is_replaced() {
        let answered = native_page(vec![
            text_turn(Some("user"), ReferenceTurnRole::User, "ask"),
            tool_turn("assistant-early", "call-early", "file", ReferenceToolStatus::Completed),
            text_turn(Some("later"), ReferenceTurnRole::User, "and then"),
        ]);
        let (replace_from, turns, next, _) = expect_delta(&answered, &anchor(&answered, "assistant-early"));
        assert_eq!(replace_from, "assistant-early");
        assert_eq!(turns, answered.turns[1..]);
        assert!(matches!(
            &turns[0].parts[0],
            ReferencePart::Tool { output, call_id, .. }
                if output == "file" && call_id.as_deref() == Some("call-early")
        ));
        assert_eq!(next.anchor_turn_id, "later");

        let still_open = native_page(vec![
            text_turn(Some("user"), ReferenceTurnRole::User, "ask"),
            tool_turn("assistant-early", "call-early", "", ReferenceToolStatus::Running),
            text_turn(Some("later"), ReferenceTurnRole::User, "and then"),
        ]);
        let (replace_from, turns, next, _) =
            expect_delta(&still_open, &anchor(&still_open, "later"));
        assert_eq!(replace_from, "assistant-early");
        assert_eq!(turns, still_open.turns[1..]);
        assert_eq!(next.anchor_turn_id, "assistant-early");
    }

    #[test]
    fn generation_change_resets() {
        let page = native_page(vec![text_turn(Some("stable"), ReferenceTurnRole::User, "ask")]);
        let mut after = anchor(&page, "stable");
        after.generation = "gen-old".into();
        assert_eq!(
            expect_reset(&page, &after),
            ReferenceHistoryResetReason::GenerationChanged
        );
    }

    #[test]
    fn missing_anchor_resets() {
        let page = native_page(vec![text_turn(Some("stable"), ReferenceTurnRole::User, "ask")]);
        let after = anchor(&page, "missing");
        assert_eq!(
            expect_reset(&page, &after),
            ReferenceHistoryResetReason::AnchorNotTurnStart
        );
    }

    #[test]
    fn window_slid_past_anchor_resets() {
        let mut paged = native_page(vec![text_turn(Some("tail"), ReferenceTurnRole::User, "now")]);
        paged.has_more = true;
        paged.cursor = Some(ReferenceHistoryCursor {
            stream_id: STREAM.into(),
            offset: 40,
        });
        assert_eq!(
            expect_reset(&paged, &anchor(&paged, "fallen-off")),
            ReferenceHistoryResetReason::AnchorOutsideWindow
        );

        let mut capped = native_page(vec![text_turn(Some("tail"), ReferenceTurnRole::User, "now")]);
        capped.unavailable_reason = Some("older history lies beyond the read cap".into());
        assert_eq!(
            expect_reset(&capped, &anchor(&capped, "fallen-off")),
            ReferenceHistoryResetReason::AnchorOutsideWindow
        );
    }

    #[test]
    fn unaligned_reader_resets() {
        let page = native_page(vec![text_turn(None, ReferenceTurnRole::User, "ask")]);
        let after = ReferenceHistoryAnchor {
            stream_id: STREAM.into(),
            generation: page.generation.clone(),
            anchor_turn_id: "missing".into(),
            previous_turn_fingerprint: None,
        };
        assert_eq!(
            expect_reset(&page, &after),
            ReferenceHistoryResetReason::UnalignedTurns
        );
    }

    #[test]
    fn oversized_delta_resets() {
        let turns = (0..=REFERENCE_HISTORY_DELTA_MAX_TURNS)
            .map(|index| text_turn(Some(&index.to_string()), ReferenceTurnRole::User, "t"))
            .collect();
        let page = native_page(turns);
        assert_eq!(
            expect_reset(&page, &anchor(&page, "0")),
            ReferenceHistoryResetReason::TooFarBehind
        );

        let huge = "x".repeat(REFERENCE_HISTORY_DELTA_MAX_BYTES);
        let page = native_page(vec![text_turn(Some("huge"), ReferenceTurnRole::Assistant, &huge)]);
        assert_eq!(
            expect_reset(&page, &anchor(&page, "huge")),
            ReferenceHistoryResetReason::TooFarBehind
        );
    }

    #[test]
    fn previous_fingerprint_mismatch_resets() {
        let page = native_page(vec![
            text_turn(Some("user"), ReferenceTurnRole::User, "ask"),
            text_turn(Some("open"), ReferenceTurnRole::Assistant, "growing"),
        ]);
        let mut after = anchor(&page, "open");
        after.previous_turn_fingerprint = Some("nope".into());
        assert_eq!(
            expect_reset(&page, &after),
            ReferenceHistoryResetReason::AnchorPrevMismatch
        );
    }

    #[test]
    fn not_native_resets() {
        let mut page = native_page(vec![text_turn(Some("t"), ReferenceTurnRole::User, "ask")]);
        page.source = ReferenceHistorySource::Scrollback;
        page.availability = ReferenceHistoryAvailability::Scrollback;
        assert_eq!(
            expect_reset(&page, &anchor(&page, "t")),
            ReferenceHistoryResetReason::NotNative
        );
    }

    #[test]
    fn unsupported_projection_resets() {
        let mut page = native_page(vec![text_turn(Some("t"), ReferenceTurnRole::User, "ask")]);
        page.source = ReferenceHistorySource::PiTranscript;
        assert_eq!(
            expect_reset(&page, &anchor(&page, "t")),
            ReferenceHistoryResetReason::UnsupportedProjection
        );
    }

    #[test]
    fn cursor_and_after_is_refused() {
        let page = native_page(vec![text_turn(Some("stable"), ReferenceTurnRole::User, "ask")]);
        let cursor = ReferenceHistoryCursor {
            stream_id: STREAM.into(),
            offset: 1,
        };
        let error = reference_history_negotiate(&page, STREAM, Some(&cursor), Some(&anchor(&page, "stable")))
            .expect_err("cursor and after");
        assert_eq!(error.code, ScopeErrorCode::InvalidRequest);
        assert!(!error.retryable);
        assert_eq!(REFERENCE_HISTORY_CURSOR_AND_AFTER, "INVALID_REQUEST");
        assert!(error.message.contains(REFERENCE_HISTORY_CURSOR_AND_AFTER));
    }

    #[test]
    fn foreign_stream_is_target_expired() {
        let page = native_page(vec![text_turn(Some("stable"), ReferenceTurnRole::User, "ask")]);
        let mut after = anchor(&page, "stable");
        after.stream_id = "other-stream".into();
        let error = reference_history_negotiate(&page, STREAM, None, Some(&after)).expect_err("foreign");
        assert_eq!(error.code, ScopeErrorCode::TargetExpired);
        assert!(error.message.contains("other-stream"));
        assert!(error.message.contains(STREAM));
    }

    #[test]
    fn no_after_returns_the_rev2_page_byte_identical() {
        let page = native_page(vec![text_turn(None, ReferenceTurnRole::User, "same")]);
        let cursor = ReferenceHistoryCursor {
            stream_id: STREAM.into(),
            offset: 8,
        };
        let read = reference_history_negotiate(&page, STREAM, Some(&cursor), None)
            .expect("rev2 query");
        let ReferenceHistoryRead::Page(returned) = read else {
            panic!("rev2 query returned a negotiated body");
        };
        assert!(std::ptr::eq(returned, &page));
        assert_eq!(
            serde_json::to_vec(returned).expect("serializes"),
            serde_json::to_vec(&page).expect("serializes")
        );
        let rendered = serde_json::to_string(returned).expect("serializes");
        assert!(!rendered.contains("\"mode\""));
        assert!(!rendered.contains("\"id\""));
    }
}
