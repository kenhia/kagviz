//! The calls document: the payload half of the events.
//!
//! `kagviz show <id> --calls` emits one JSON object per session holding what
//! each tool call actually said — its input, and the result text as the model
//! was handed it. The events document deliberately carries neither, because
//! "this document would be the transcript again"; this is that text, in a
//! fourth document under the same contract rules, joined to the events by
//! `tool_use_id` so a consumer fetches it only when a row is expanded.
//!
//! **It is the one document kagviz does not write by default.** Everything
//! else served is derived — counts, durations, classifications, capped
//! previews — and this is raw session content: file contents, command output,
//! pasted material, and potentially credentials. `derive` writes it only when
//! asked (`--calls`), so the served tree carries it only because someone
//! decided it should. See `docs/facts-contract.md` and sprint 015.
//!
//! Counted, never inferred — and here, *copied*, never summarised. Every
//! field is read off the transcript by the same `Counter` that produces the
//! facts and the events, in the same loop iteration, which is what makes
//! `input` and `result` agree with the events' `input_bytes` and
//! `result_bytes` by construction rather than by assertion.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One session's call payloads: the session's own tier and every spawn's,
/// **flattened into one list**.
///
/// Flat on purpose. The join key is the `tool_use` id, which is unique across
/// the session, so a consumer expanding a delegated agent's row joins exactly
/// as it does for the parent's. Mirroring the events' `spawns[]` split would
/// force a consumer to know which tier a row came from before it could look
/// up the text — strictly worse, for a document whose only job is "give me
/// what this id said".
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Calls {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// One entry per `tool` event across the events document's own tier and
    /// all of its spawns: the session's tier first, in the order the calls
    /// were made, then each spawn in `delegation.spawns[]` order.
    pub calls: Vec<Call>,
}

/// What one tool call said, and what came back.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Call {
    /// The `tool_use` id — the join key back to the events document's `tool`
    /// events. **Absent** when the transcript carried none, and then this
    /// entry cannot be joined to anything: it is still here so the document
    /// does not under-report the calls that were made.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The tool's name. Duplicated from the event on purpose — an entry with
    /// no `id` has nothing to join to and would otherwise be nameless, and it
    /// is what makes this document readable on its own with `jq`. Read off
    /// the same block as the event's, in the same iteration, so the two
    /// cannot disagree.
    pub tool: String,
    /// The call's input, as JSON, exactly as the model was handed it.
    /// **Absent** when the block carried none — and then so is the events
    /// document's `input_bytes`.
    ///
    /// The invariant: `to_string(input).len()` **is** the event's
    /// `input_bytes`, because that is how `input_bytes` is computed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    /// The result's text as the model was handed it — the string form, or the
    /// `text` blocks of the array form concatenated in order.
    ///
    /// **Absent** when no result arrived: an interrupted call, or one still
    /// running when the transcript ends. Present and **empty** when a result
    /// arrived carrying no text. Those are different readings and the
    /// document keeps them apart, exactly as the events document does with
    /// `result_at`/`result_bytes`.
    ///
    /// The invariant: `result.len()` **is** the event's `result_bytes`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Block types in the result that carried no text, in the order they
    /// appeared — `image` and friends.
    ///
    /// Present only when there were any, and it is the reason an empty
    /// `result` never has to be read as an empty result: a screenshot comes
    /// back as `result: ""` plus `result_blocks: ["image"]`. The events
    /// document's `result_bytes` counts the same zero and has no way to say
    /// this; the rule that a zero must not stand in for an unknown is why
    /// this field exists.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub result_blocks: Vec<String>,
    /// The result the model saw is a `<persisted-output>` placeholder: the
    /// harness judged the real output too large for the context, offloaded it
    /// to `<session-id>/tool-results/<id>.txt`, and handed the model a path
    /// and a ~2 KB preview instead.
    ///
    /// Present only when true. Read from the harness's own
    /// `toolUseResult.persistedOutputPath` rather than by matching the shape
    /// of the text — the harness says what it wrote, and that beats a regex
    /// over it. **`result` is the preview, not the output**, and a consumer
    /// that does not say so lets a reader take 2 KB for the whole thing.
    #[serde(skip_serializing_if = "is_false")]
    pub persisted: bool,
    /// UTF-8 bytes of the offloaded output, as the harness recorded them in
    /// `toolUseResult.persistedOutputSize`. Absent when it recorded a path
    /// but no size — an unknown, not a zero. The file itself is **not**
    /// served; only this number says how much `result` is a preview of.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persisted_bytes: Option<u64>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// The text of a `tool_result` block's content, and the block types that
/// contributed none of it.
///
/// The text half is [`crate::events::text_bytes`]'s own reading — the string
/// form, or the `text` blocks of the array form in order — so the string this
/// returns is exactly as long as the event's `result_bytes`. The second half
/// is what `result_bytes` has no room to say: an image contributes zero
/// bytes, and a zero that means "there was nothing" and a zero that means
/// "there was something kagviz does not carry" are different readings.
pub(crate) fn result_text(content: Option<&Value>) -> (String, Vec<String>) {
    match content {
        Some(Value::String(s)) => (s.clone(), Vec::new()),
        Some(Value::Array(blocks)) => {
            let mut text = String::new();
            let mut other = Vec::new();
            for b in blocks {
                let kind = b.get("type").and_then(Value::as_str).unwrap_or_default();
                match (kind, b.get("text").and_then(Value::as_str)) {
                    ("text", Some(t)) => text.push_str(t),
                    // A `text` block with no `text` contributes no bytes and
                    // is not a different kind of thing: `text_bytes` skips it
                    // the same way, so naming it here would be noise.
                    ("text", None) => {}
                    _ => other.push(kind.to_string()),
                }
            }
            (text, other)
        }
        // No content is an empty result — a known zero, and the same reading
        // `text_bytes` gives it.
        _ => (String::new(), Vec::new()),
    }
}

/// What the harness recorded about an offloaded result: whether it offloaded
/// one at all, and how large it said the real output was.
///
/// `persistedOutputPath` is the discriminator because it is the harness
/// naming what it did. The path itself is never carried — it points into a
/// mirror that is not served, and on a Windows-written transcript it is a
/// local `C:\Users\…` path that would mean nothing to a reader.
pub(crate) fn persisted(result: Option<&Value>) -> (bool, Option<u64>) {
    let Some(r) = result else {
        return (false, None);
    };
    if r.get("persistedOutputPath").is_none() {
        return (false, None);
    }
    (true, r.get("persistedOutputSize").and_then(Value::as_u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::text_bytes;

    /// The invariant the whole document rests on: this text is exactly as
    /// long as the events document says the result was.
    #[test]
    fn result_text_is_exactly_as_long_as_the_events_say() {
        for content in [
            Value::String("héllo".into()),
            serde_json::json!([
                {"type": "text", "text": "abc"},
                {"type": "image", "source": {"data": "AAAA"}},
                {"type": "text", "text": "dé"}
            ]),
            serde_json::json!([]),
        ] {
            let (text, _) = result_text(Some(&content));
            assert_eq!(text.len(), text_bytes(Some(&content)), "{content}");
        }
        assert_eq!(result_text(None).0.len(), text_bytes(None));
    }

    #[test]
    fn a_block_that_carried_no_text_is_named_rather_than_counted_as_nothing() {
        let content = serde_json::json!([
            {"type": "image", "source": {"data": "AAAA"}},
            {"type": "tool_reference", "id": "x"}
        ]);
        let (text, blocks) = result_text(Some(&content));
        assert_eq!(text, "", "an image carries no text");
        assert_eq!(
            blocks,
            ["image", "tool_reference"],
            "and the empty string must not be the only thing a reader sees"
        );
    }

    #[test]
    fn a_text_block_with_no_text_is_not_reported_as_another_kind() {
        let content = serde_json::json!([{"type": "text"}, {"type": "text", "text": "a"}]);
        assert_eq!(result_text(Some(&content)), ("a".into(), Vec::new()));
    }

    #[test]
    fn offloading_is_read_from_the_harness_not_from_the_text() {
        let r = serde_json::json!({
            "persistedOutputPath": "C:\\Users\\x\\tool-results\\a.txt",
            "persistedOutputSize": 348_112
        });
        assert_eq!(persisted(Some(&r)), (true, Some(348_112)));

        // A path with no size is an unknown, not a zero.
        let r = serde_json::json!({"persistedOutputPath": "/x/a.txt"});
        assert_eq!(persisted(Some(&r)), (true, None));

        // Text that merely looks like a placeholder is not the harness saying so.
        let r = serde_json::json!({"stdout": "<persisted-output path=…>"});
        assert_eq!(persisted(Some(&r)), (false, None));
        assert_eq!(persisted(None), (false, None));
    }

    /// The flags-and-absences convention, on the field that carries the most
    /// weight: an interrupted call has no `result` key at all, and a result
    /// that came back empty has one holding `""`.
    #[test]
    fn an_interrupted_call_and_an_empty_result_do_not_serialize_alike() {
        let interrupted = Call {
            id: Some("t1".into()),
            tool: "Bash".into(),
            input: Some(serde_json::json!({"command": "true"})),
            ..Call::default()
        };
        let empty = Call {
            result: Some(String::new()),
            ..interrupted.clone()
        };
        let a = serde_json::to_string(&interrupted).unwrap();
        let b = serde_json::to_string(&empty).unwrap();
        assert!(!a.contains("result"), "{a}");
        assert!(b.contains(r#""result":"""#), "{b}");
        assert!(!a.contains("null") && !b.contains("null"), "{a} / {b}");
        assert_eq!(serde_json::from_str::<Call>(&a).unwrap(), interrupted);
        assert_eq!(serde_json::from_str::<Call>(&b).unwrap(), empty);
    }
}
