//! The deterministic pass: transcript records in, counted facts out.
//!
//! Everything here is a pure function of the bytes on disk. The same
//! transcript yields the same summary forever, which is the whole point — any
//! model-written narrative sits *on top* of this, never inside it.

use crate::discover::SessionPaths;
use crate::transcript::{Content, INJECTED_PREFIXES, Transcript};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Gaps at or above this are counted as idle, not work.
///
/// Wall-clock span is close to meaningless on its own: a resumed session can
/// span days while holding well under an hour of actual work. Reporting both
/// is the honest form.
pub const IDLE_GAP_SECS: i64 = 120;

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct TokenTotals {
    pub input: u64,
    pub output: u64,
    pub thinking: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

/// What a file-modifying tool did, as recovered from its result payload.
#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct FileChanges {
    pub files_touched: usize,
    pub lines_added: usize,
    pub lines_deleted: usize,
    /// Tool calls that plainly could have changed files but exposed no
    /// recoverable diff.
    ///
    /// `Bash` heredocs and `sed` edits land here, and so does any MCP editor
    /// with its own result shape. Surfaced rather than hidden, because a zero
    /// that means "nothing changed" and a zero that means "kagviz could not
    /// see it" are very different readings.
    pub opaque_edits: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub session_id: Option<String>,
    pub project: Option<String>,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub cli_versions: BTreeSet<String>,
    pub models: BTreeMap<String, u32>,

    pub started: Option<DateTime<Utc>>,
    pub ended: Option<DateTime<Utc>>,
    pub wall_secs: i64,
    pub active_secs: i64,
    pub idle_secs: i64,

    pub records: usize,
    pub skipped_lines: usize,
    pub assistant_turns: usize,
    pub user_prompts: usize,
    pub pasted_attachments: usize,
    pub ask_user_questions: usize,
    pub skills: Vec<String>,
    pub subagents: Vec<String>,
    pub subagent_transcripts: usize,

    pub tool_calls: BTreeMap<String, u32>,
    pub tool_failures: BTreeMap<String, u32>,
    pub tokens: TokenTotals,
    pub changes: FileChanges,
}

impl Summary {
    pub fn total_tool_calls(&self) -> u32 {
        self.tool_calls.values().sum()
    }

    pub fn total_tool_failures(&self) -> u32 {
        self.tool_failures.values().sum()
    }
}

/// Tools that routinely change files without exposing a diff kagviz can read.
fn may_edit_opaquely(tool: &str) -> bool {
    matches!(tool, "Bash" | "PowerShell")
}

pub fn summarize(paths: Option<&SessionPaths>, transcript: &Transcript) -> Summary {
    let records = &transcript.records;
    let mut s = Summary {
        skipped_lines: transcript.skipped,
        records: records.len(),
        project: paths.map(|p| p.project.clone()),
        subagent_transcripts: paths.map_or(0, |p| p.subagents.len()),
        ..Summary::default()
    };

    // tool_use id -> tool name, so an is_error result can be blamed correctly.
    let mut tool_names: BTreeMap<String, String> = BTreeMap::new();
    let mut stamps: Vec<DateTime<Utc>> = Vec::new();
    let mut changed_files: BTreeSet<String> = BTreeSet::new();

    for rec in records {
        if s.session_id.is_none() {
            s.session_id.clone_from(&rec.session_id);
        }
        if let Some(v) = &rec.version {
            s.cli_versions.insert(v.clone());
        }
        if s.cwd.is_none() {
            s.cwd.clone_from(&rec.cwd);
        }
        if s.git_branch.is_none() {
            s.git_branch.clone_from(&rec.git_branch);
        }
        if let Some(ts) = rec.timestamp.as_deref().and_then(parse_stamp) {
            stamps.push(ts);
        }

        if rec.kind == "assistant"
            && let Some(msg) = &rec.message
        {
            s.assistant_turns += 1;
            if let Some(model) = &msg.model {
                *s.models.entry(model.clone()).or_default() += 1;
            }
            if let Some(u) = msg.usage {
                s.tokens.input += u.input_tokens;
                s.tokens.output += u.output_tokens;
                s.tokens.thinking += u.output_tokens_details.thinking_tokens;
                s.tokens.cache_read += u.cache_read_input_tokens;
                s.tokens.cache_write += u.cache_creation_input_tokens;
            }
            for block in msg.content.blocks() {
                if block.kind != "tool_use" {
                    continue;
                }
                let Some(name) = block.name.clone() else {
                    continue;
                };
                *s.tool_calls.entry(name.clone()).or_default() += 1;
                if let Some(id) = &block.id {
                    tool_names.insert(id.clone(), name.clone());
                }
                match name.as_str() {
                    "AskUserQuestion" => s.ask_user_questions += 1,
                    "Skill" => push_input_str(&mut s.skills, block.input.as_ref(), "skill"),
                    "Agent" | "Task" => {
                        push_input_str(&mut s.subagents, block.input.as_ref(), "subagent_type");
                    }
                    _ => {}
                }
            }
        }

        if rec.kind == "user"
            && let Some(msg) = &rec.message
        {
            if is_user_turn(&msg.content) {
                s.user_prompts += 1;
            }
            s.pasted_attachments += msg
                .content
                .blocks()
                .iter()
                .filter(|b| matches!(b.kind.as_str(), "image" | "document"))
                .count();
            for block in msg.content.blocks() {
                if block.kind == "tool_result" && block.is_error.unwrap_or(false) {
                    let name = block
                        .tool_use_id
                        .as_ref()
                        .and_then(|id| tool_names.get(id))
                        .cloned()
                        .unwrap_or_else(|| "<unknown>".to_string());
                    *s.tool_failures.entry(name).or_default() += 1;
                }
            }
        }

        if let Some(result) = &rec.tool_use_result {
            tally_changes(result, &mut s.changes, &mut changed_files);
        }
    }

    // Opaque edits: shell tools that ran at all. Counted as "could have
    // changed files unseen", never as changes.
    s.changes.opaque_edits = s
        .tool_calls
        .iter()
        .filter(|(name, _)| may_edit_opaquely(name))
        .map(|(_, n)| *n as usize)
        .sum();
    s.changes.files_touched = changed_files.len();

    s.skills.sort();
    s.skills.dedup();
    s.subagents.sort();

    stamps.sort_unstable();
    if let (Some(first), Some(last)) = (stamps.first(), stamps.last()) {
        s.started = Some(*first);
        s.ended = Some(*last);
        s.wall_secs = (*last - *first).num_seconds();
        s.idle_secs = stamps
            .windows(2)
            .map(|w| (w[1] - w[0]).num_seconds())
            .filter(|gap| *gap >= IDLE_GAP_SECS)
            .sum();
        s.active_secs = s.wall_secs - s.idle_secs;
    }

    s
}

/// Whether a `user` record is the user actually saying something.
///
/// Three different things share the user channel: real prompts, tool results,
/// and text the harness injects (IDE state, slash-command scaffolding, system
/// reminders). Only the first is user involvement, and telling them apart is
/// the whole job — `promptId` is present on all three.
fn is_user_turn(content: &Content) -> bool {
    match content {
        Content::Text(text) => {
            let text = text.trim_start();
            !text.is_empty() && !INJECTED_PREFIXES.iter().any(|p| text.starts_with(p))
        }
        Content::Blocks(blocks) => {
            if blocks.iter().any(|b| b.kind == "tool_result") {
                return false;
            }
            blocks.iter().any(|b| match b.kind.as_str() {
                // A pasted image or PDF is the user acting, even without text.
                "image" | "document" => true,
                "text" => {
                    !b.is_injected_context()
                        && b.text.as_deref().is_some_and(|t| !t.trim().is_empty())
                }
                _ => false,
            })
        }
        Content::Empty => false,
    }
}

fn push_input_str(into: &mut Vec<String>, input: Option<&Value>, key: &str) {
    if let Some(v) = input.and_then(|i| i.get(key)).and_then(Value::as_str) {
        into.push(v.to_string());
    }
}

fn parse_stamp(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Recover line deltas from a tool result that carries a `structuredPatch`.
///
/// A `create` result has an empty patch and the whole file in `content`, so
/// its line count is the addition.
fn tally_changes(result: &Value, changes: &mut FileChanges, files: &mut BTreeSet<String>) {
    let Some(patch) = result.get("structuredPatch") else {
        return;
    };
    if let Some(path) = result.get("filePath").and_then(Value::as_str) {
        files.insert(path.to_string());
    }

    let mut saw_hunk = false;
    if let Some(hunks) = patch.as_array() {
        for hunk in hunks {
            let Some(lines) = hunk.get("lines").and_then(Value::as_array) else {
                continue;
            };
            saw_hunk = true;
            for line in lines.iter().filter_map(Value::as_str) {
                match line.as_bytes().first() {
                    Some(b'+') => changes.lines_added += 1,
                    Some(b'-') => changes.lines_deleted += 1,
                    _ => {}
                }
            }
        }
    }

    if !saw_hunk
        && result.get("type").and_then(Value::as_str) == Some("create")
        && let Some(body) = result.get("content").and_then(Value::as_str)
    {
        changes.lines_added += body.lines().count();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transcript(lines: &[&str]) -> Transcript {
        Transcript {
            records: lines
                .iter()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect(),
            skipped: 0,
        }
    }

    #[test]
    fn idle_gaps_are_excluded_from_active_time() {
        // 0s -> 30s (active) -> 2h later (idle) -> +10s (active)
        let t = transcript(&[
            r#"{"type":"user","timestamp":"2026-08-20T10:00:00.000Z"}"#,
            r#"{"type":"assistant","timestamp":"2026-08-20T10:00:30.000Z"}"#,
            r#"{"type":"user","timestamp":"2026-08-20T12:00:30.000Z"}"#,
            r#"{"type":"assistant","timestamp":"2026-08-20T12:00:40.000Z"}"#,
        ]);
        let s = summarize(None, &t);
        assert_eq!(s.wall_secs, 7240);
        assert_eq!(s.idle_secs, 7200);
        assert_eq!(s.active_secs, 40);
    }

    #[test]
    fn failures_are_blamed_on_the_tool_that_was_called() {
        let t = transcript(&[
            r#"{"type":"assistant","message":{"content":[
                {"type":"tool_use","id":"t1","name":"Bash"},
                {"type":"tool_use","id":"t2","name":"Read"}]}}"#,
            r#"{"type":"user","message":{"content":[
                {"type":"tool_result","tool_use_id":"t1","is_error":true},
                {"type":"tool_result","tool_use_id":"t2"}]}}"#,
        ]);
        let s = summarize(None, &t);
        assert_eq!(s.total_tool_calls(), 2);
        assert_eq!(s.total_tool_failures(), 1);
        assert_eq!(s.tool_failures.get("Bash"), Some(&1));
        assert!(!s.tool_failures.contains_key("Read"));
    }

    #[test]
    fn real_prompts_are_counted_but_tool_results_are_not() {
        let t = transcript(&[
            r#"{"type":"user","promptId":"p1","message":{"content":"do the thing"}}"#,
            r#"{"type":"user","promptId":"p1","message":{"content":[
                {"type":"text","text":"and this too"}]}}"#,
            r#"{"type":"user","promptId":"p1","message":{"content":[
                {"type":"tool_result","tool_use_id":"t1"}]}}"#,
        ]);
        assert_eq!(summarize(None, &t).user_prompts, 2);
    }

    /// `promptId` rides on harness-injected records too, so it cannot be the
    /// discriminator. Everything below is the harness talking, not the user.
    #[test]
    fn harness_injected_context_is_not_user_involvement() {
        let t = transcript(&[
            r#"{"type":"user","promptId":"p1","message":{"content":
                "<local-command-caveat>Caveat: the messages below"}}"#,
            r#"{"type":"user","promptId":"p1","message":{"content":
                "<command-name>/model</command-name>"}}"#,
            r#"{"type":"user","promptId":"p1","message":{"content":
                "[Image: original 2160x2880, displayed at 1500x2000.]"}}"#,
            r#"{"type":"user","promptId":"p1","message":{"content":[
                {"type":"text","text":"<ide_opened_file>The user opened a file"}]}}"#,
            r#"{"type":"user","promptId":"p1","message":{"content":[
                {"type":"text","text":"<system-reminder>be good</system-reminder>"}]}}"#,
        ]);
        assert_eq!(summarize(None, &t).user_prompts, 0);
    }

    #[test]
    fn a_pasted_image_counts_as_the_user_acting() {
        let t = transcript(&[
            r#"{"type":"user","promptId":"p1","message":{"content":[
                {"type":"image","source":{}},
                {"type":"text","text":"what is this?"}]}}"#,
            r#"{"type":"user","promptId":"p2","message":{"content":[
                {"type":"document","source":{}}]}}"#,
        ]);
        let s = summarize(None, &t);
        assert_eq!(s.user_prompts, 2);
        assert_eq!(s.pasted_attachments, 2);
    }

    #[test]
    fn structured_patch_line_deltas_are_counted() {
        let t = transcript(&[
            r#"{"type":"user","toolUseResult":{"filePath":"/a.rs","structuredPatch":[
                {"lines":[" ctx","+added","+added2","-gone"]}]}}"#,
            r#"{"type":"user","toolUseResult":{"filePath":"/b.rs","type":"create",
                "structuredPatch":[],"content":"one\ntwo\nthree"}}"#,
        ]);
        let s = summarize(None, &t);
        assert_eq!(s.changes.lines_added, 5);
        assert_eq!(s.changes.lines_deleted, 1);
        assert_eq!(s.changes.files_touched, 2);
    }

    #[test]
    fn shell_calls_are_reported_as_opaque_rather_than_as_no_change() {
        let t = transcript(&[r#"{"type":"assistant","message":{"content":[
                {"type":"tool_use","id":"t1","name":"Bash"}]}}"#]);
        let s = summarize(None, &t);
        assert_eq!(s.changes.files_touched, 0);
        assert_eq!(s.changes.opaque_edits, 1);
    }

    #[test]
    fn user_involvement_and_delegation_are_picked_up() {
        let t = transcript(&[r#"{"type":"assistant","message":{"content":[
                {"type":"tool_use","id":"t1","name":"AskUserQuestion","input":{}},
                {"type":"tool_use","id":"t2","name":"Skill","input":{"skill":"sprint-ship"}},
                {"type":"tool_use","id":"t3","name":"Agent","input":{"subagent_type":"Explore"}}]}}"#]);
        let s = summarize(None, &t);
        assert_eq!(s.ask_user_questions, 1);
        assert_eq!(s.skills, vec!["sprint-ship"]);
        assert_eq!(s.subagents, vec!["Explore"]);
    }
}
