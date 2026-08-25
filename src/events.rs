//! The events document: the detail tier under the facts.
//!
//! `kagviz show <id> --events` emits one JSON object per session holding
//! every assistant turn and every tool call, in time order, each joined to
//! the phase it fell in. It is what a click on the timeline reads: the facts
//! carry the *counts* a bucket or a phase adds up to, and this carries the
//! things that were counted. A separate document under the same contract
//! discipline — so the facts stay light and a front-end fetches the leaf on
//! demand — and the reason the `MAX_BUCKETS` ceiling did not need raising:
//! a consumer that wants finer buckets derives them from these timestamps.
//! See `docs/facts-contract.md`.
//!
//! Counted, never inferred. Every field is read off the transcript by the
//! same `Counter` that produces the facts, so the two documents cannot
//! disagree about how many tool calls there were or which of them failed.
//! The types live here; the pass that fills them is in `summary.rs`, because
//! it *is* that pass.

use crate::summary::{TokenTotals, ToolClass};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One session's events, both tiers.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Events {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// The session's own tier, in the order the facts' `activity` and
    /// `phases` were cut from: by time, ties in transcript order, then any
    /// event whose record carried no timestamp.
    pub events: Vec<Event>,
    /// One entry per `delegation.spawns[]` in the facts, in the same order.
    pub spawns: Vec<SpawnEvents>,
}

/// One delegated agent's events. No `phase` on any of them — phases cut the
/// *parent's* timeline, and a concurrent agent has no position on it.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SpawnEvents {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub events: Vec<Event>,
}

/// One thing that happened, tagged by `kind`.
///
/// A `turn` is an assistant message — what it cost, and how many tool calls
/// it made. A `tool` is one of those calls, joined to its result. A turn's
/// tool events follow it directly, in the order the message listed them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Turn {
        #[serde(skip_serializing_if = "Option::is_none")]
        at: Option<DateTime<Utc>>,
        /// Index into the facts' `phases`. Absent on a spawn's events, and on
        /// a record with no timestamp — which no phase can hold.
        #[serde(skip_serializing_if = "Option::is_none")]
        phase: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// This turn's usage. Absent when the record carried none.
        #[serde(skip_serializing_if = "Option::is_none")]
        tokens: Option<TokenTotals>,
        /// Tool calls this turn made: the `tool` events that follow it.
        #[serde(default)]
        tools: u32,
    },
    Tool {
        #[serde(skip_serializing_if = "Option::is_none")]
        at: Option<DateTime<Utc>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        phase: Option<usize>,
        tool: String,
        /// How the phase mix counted it — the same table, so a consumer that
        /// colours by class agrees with the facts' `kind`.
        class: ToolClass,
        /// The `tool_use` id, for joining back to the raw transcript.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// The call's input re-serialized compactly with sorted keys — a
        /// canonical size, not the on-disk one. Absent when there was none.
        #[serde(skip_serializing_if = "Option::is_none")]
        input_bytes: Option<usize>,
        /// When the result was recorded. Absent when none was — an
        /// interrupted call, or one still running when the transcript ends.
        #[serde(skip_serializing_if = "Option::is_none")]
        result_at: Option<DateTime<Utc>>,
        /// The result came back `is_error`. Present only when true.
        #[serde(default, skip_serializing_if = "is_false")]
        failed: bool,
        /// UTF-8 bytes of the result's text as the model was handed it — an
        /// offloaded result counts its placeholder and preview, which is
        /// what the model saw. Absent when no result arrived.
        #[serde(skip_serializing_if = "Option::is_none")]
        result_bytes: Option<usize>,
        /// Files the call changed, as the tool named them. Present only when
        /// the result named any.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<String>,
        /// Line deltas read out of the result's diff. Absent when the result
        /// carried no diff kagviz could read — see `opaque`.
        #[serde(skip_serializing_if = "Option::is_none")]
        lines_added: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        lines_deleted: Option<usize>,
        /// This call is one of the facts' `changes.opaque_edits`: it could
        /// have changed files and exposed no line counts. Present only when
        /// true.
        #[serde(default, skip_serializing_if = "is_false")]
        opaque: bool,
    },
}

impl Event {
    /// Set the phase — the one field the pass that builds an event cannot
    /// know until the whole session has been cut.
    pub(crate) fn set_phase(&mut self, index: Option<usize>) {
        match self {
            Event::Turn { phase, .. } | Event::Tool { phase, .. } => *phase = index,
        }
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// UTF-8 bytes of a `tool_result` block's text: the string form, or the
/// `text` blocks of the array form summed. Images and other block types
/// contribute nothing — they are tokens, not text, and counting their
/// base64 would say nothing a reader could use. No `content` is an empty
/// result, which is a known zero.
pub(crate) fn text_bytes(content: Option<&Value>) -> usize {
    match content {
        Some(Value::String(s)) => s.len(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .map(str::len)
            .sum(),
        _ => 0,
    }
}

/// Canonical byte size of a tool call's input.
pub(crate) fn input_bytes(input: Option<&Value>) -> Option<usize> {
    input.map(|v| v.to_string().len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_text_is_measured_in_either_form_and_images_count_nothing() {
        assert_eq!(text_bytes(Some(&Value::String("héllo".into()))), 6);
        let blocks = serde_json::json!([
            {"type": "text", "text": "abc"},
            {"type": "image", "source": {"data": "AAAAAAAAAAAAAAAA"}},
            {"type": "text", "text": "de"}
        ]);
        assert_eq!(text_bytes(Some(&blocks)), 5);
        assert_eq!(text_bytes(None), 0, "no content is an empty result");
    }

    #[test]
    fn input_size_is_canonical_not_on_disk() {
        // Keys sorted, no whitespace: the same bytes however the CLI wrote it.
        let a = serde_json::json!({"b": 1, "a": "x"});
        let b: Value = serde_json::from_str("{ \"a\" : \"x\" ,  \"b\":1 }").unwrap();
        assert_eq!(input_bytes(Some(&a)), input_bytes(Some(&b)));
        assert_eq!(input_bytes(Some(&a)), Some(r#"{"a":"x","b":1}"#.len()));
        assert_eq!(input_bytes(None), None);
    }

    /// The flags-when-true convention: a successful, readable call carries
    /// neither `failed` nor `opaque`, and they read back as false.
    #[test]
    fn false_flags_are_absent_and_read_back_false() {
        let e = Event::Tool {
            at: None,
            phase: None,
            tool: "Read".into(),
            class: ToolClass::Read,
            id: Some("t1".into()),
            input_bytes: Some(10),
            result_at: None,
            failed: false,
            result_bytes: Some(3),
            files: vec![],
            lines_added: None,
            lines_deleted: None,
            opaque: false,
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(!json.contains("failed"), "{json}");
        assert!(!json.contains("opaque"), "{json}");
        assert!(!json.contains("files"), "{json}");
        assert!(!json.contains("null"), "{json}");
        assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), e);
    }
}
