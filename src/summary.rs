//! The deterministic pass: transcript records in, counted facts out.
//!
//! Everything here is a pure function of the bytes on disk. The same
//! transcript yields the same summary forever, which is the whole point — any
//! model-written narrative sits *on top* of this, never inside it.

use crate::discover::SessionPaths;
use crate::transcript::{Block, Content, INJECTED_PREFIXES, Transcript};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Gaps at or above this are counted as idle, not work.
///
/// Wall-clock span is close to meaningless on its own: a resumed session can
/// span days while holding well under an hour of actual work. Reporting both
/// is the honest form.
pub const IDLE_GAP_SECS: i64 = 120;

/// Bucket widths the activity series will choose between, smallest first.
///
/// The width is a property of the session, not of the renderer: a ten-minute
/// session and a ten-hour one both have to fit the same strip, and the choice
/// belongs in the facts so two renderings of one session can never disagree.
const BUCKET_LADDER: &[i64] = &[5, 10, 15, 30, 60, 120, 300, 600, 1800];

/// Ceiling on buckets across the whole session; the ladder is walked until the
/// series fits under it.
const MAX_BUCKETS: usize = 240;

/// How much of a user turn is carried as its label.
const PREVIEW_CHARS: usize = 80;

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TokenTotals {
    pub input: u64,
    pub output: u64,
    pub thinking: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

/// What a file-modifying tool did, as recovered from its result payload.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
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

/// One column of the activity strip: what happened in a fixed slice of time.
///
/// Counts only. What the work *was* is a later question — segmentation and
/// labelling are the next sprint, and neither belongs in a bucket.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Bucket {
    pub records: u32,
    pub tool_calls: u32,
    pub tool_failures: u32,
    pub user_turns: u32,
    pub output_tokens: u64,
}

impl Bucket {
    pub fn is_empty(&self) -> bool {
        self.records == 0
    }
}

/// A stretch of continuous work, bounded by idle gaps on either side.
///
/// Splitting at [`IDLE_GAP_SECS`] is what lets the strip collapse idle: a
/// six-day resumed session becomes a handful of spans with labelled breaks
/// between them, rather than 52 minutes of work lost in a week of whitespace.
#[derive(Debug, Serialize, Deserialize)]
pub struct ActivitySpan {
    pub started: DateTime<Utc>,
    pub ended: DateTime<Utc>,
    pub secs: i64,
    /// Idle seconds between the end of the previous span and this one; `0` for
    /// the first span, which has nothing before it.
    pub idle_before_secs: i64,
    pub buckets: Vec<Bucket>,
}

/// The session as a uniformly-scaled time series with idle removed.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Activity {
    /// Seconds per bucket, chosen from [`BUCKET_LADDER`] for this session.
    pub bucket_secs: i64,
    pub spans: Vec<ActivitySpan>,
}

impl Activity {
    /// The busiest bucket's record count, for normalising a bar height.
    pub fn peak_records(&self) -> u32 {
        self.spans
            .iter()
            .flat_map(|s| &s.buckets)
            .map(|b| b.records)
            .max()
            .unwrap_or(0)
    }
}

/// A moment where the user was in the loop rather than watching.
///
/// These are the decision points of a session, and they are the one thing a
/// tool-call histogram cannot show. Prompts carry a label rather than their
/// full text; questions carry what was actually asked and chosen, because a
/// count of "3 questions" says nothing about what was decided.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Involvement {
    /// The user said something — typed text, a pasted image, or both.
    Prompt {
        at: Option<DateTime<Utc>>,
        /// The first [`PREVIEW_CHARS`] characters, whitespace collapsed.
        preview: String,
        truncated: bool,
        /// Images or documents pasted with this turn.
        attachments: usize,
    },
    /// The agent stopped and asked. `chosen` is absent when the transcript
    /// holds no answer — an interrupted question, not a silent one.
    Question {
        at: Option<DateTime<Utc>>,
        header: Option<String>,
        question: String,
        options: Vec<String>,
        chosen: Option<String>,
    },
}

impl Involvement {
    pub fn at(&self) -> Option<DateTime<Utc>> {
        match self {
            Involvement::Prompt { at, .. } | Involvement::Question { at, .. } => *at,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
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

    /// Work over time, idle removed. Additive to the facts contract: the
    /// totals above stay exactly what they were.
    pub activity: Activity,
    /// Every moment the user was involved, in transcript order.
    pub user_involvement: Vec<Involvement>,
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
    let mut events: Vec<Event> = Vec::new();
    let mut changed_files: BTreeSet<String> = BTreeSet::new();
    // AskUserQuestion tool_use id -> the involvement entries it produced, so
    // the answers on its result can be filled in when they arrive.
    let mut open_questions: BTreeMap<String, Vec<usize>> = BTreeMap::new();

    for rec in records {
        let at = rec.timestamp.as_deref().and_then(parse_stamp);
        let mut event = Event {
            at,
            ..Event::default()
        };

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
        if let Some(ts) = at {
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
                event.output_tokens += u.output_tokens;
            }
            for block in msg.content.blocks() {
                if block.kind != "tool_use" {
                    continue;
                }
                let Some(name) = block.name.clone() else {
                    continue;
                };
                *s.tool_calls.entry(name.clone()).or_default() += 1;
                event.tool_calls += 1;
                if let Some(id) = &block.id {
                    tool_names.insert(id.clone(), name.clone());
                }
                match name.as_str() {
                    "AskUserQuestion" => {
                        s.ask_user_questions += 1;
                        let asked = push_questions(&mut s.user_involvement, block, at);
                        if let Some(id) = &block.id
                            && !asked.is_empty()
                        {
                            open_questions.insert(id.clone(), asked);
                        }
                    }
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
            let attachments = msg
                .content
                .blocks()
                .iter()
                .filter(|b| matches!(b.kind.as_str(), "image" | "document"))
                .count();
            s.pasted_attachments += attachments;
            if is_user_turn(&msg.content) {
                s.user_prompts += 1;
                event.user_turns += 1;
                let (preview, truncated) = preview_of(&msg.content);
                s.user_involvement.push(Involvement::Prompt {
                    at,
                    preview,
                    truncated,
                    attachments,
                });
            }
            for block in msg.content.blocks() {
                if block.kind != "tool_result" {
                    continue;
                }
                if block.is_error.unwrap_or(false) {
                    let name = block
                        .tool_use_id
                        .as_ref()
                        .and_then(|id| tool_names.get(id))
                        .cloned()
                        .unwrap_or_else(|| "<unknown>".to_string());
                    *s.tool_failures.entry(name).or_default() += 1;
                    event.tool_failures += 1;
                }
                if let Some(id) = &block.tool_use_id
                    && let Some(indices) = open_questions.remove(id)
                {
                    fill_answers(
                        &mut s.user_involvement,
                        &indices,
                        rec.tool_use_result.as_ref(),
                    );
                }
            }
        }

        if let Some(result) = &rec.tool_use_result {
            tally_changes(result, &mut s.changes, &mut changed_files);
        }

        if event.at.is_some() {
            events.push(event);
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

    // Records are appended in order, but a transcript that merged two writers
    // need not be sorted; the series is built from time, not from file order.
    events.sort_by_key(|e| e.at);
    s.activity = build_activity(&events);

    s
}

/// One record's contribution to the activity series.
#[derive(Debug, Default, Clone, Copy)]
struct Event {
    at: Option<DateTime<Utc>>,
    tool_calls: u32,
    tool_failures: u32,
    user_turns: u32,
    output_tokens: u64,
}

/// Cut the event stream at idle gaps and bucket each resulting span.
fn build_activity(events: &[Event]) -> Activity {
    let times: Vec<DateTime<Utc>> = events.iter().filter_map(|e| e.at).collect();
    if times.is_empty() {
        return Activity::default();
    }

    // Inclusive index ranges of continuous work.
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    for i in 1..times.len() {
        if (times[i] - times[i - 1]).num_seconds() >= IDLE_GAP_SECS {
            ranges.push((start, i - 1));
            start = i;
        }
    }
    ranges.push((start, times.len() - 1));

    let bucket_secs = choose_bucket_secs(&times, &ranges);
    let mut spans = Vec::with_capacity(ranges.len());
    let mut prev_end: Option<DateTime<Utc>> = None;

    for (a, b) in ranges {
        let (started, ended) = (times[a], times[b]);
        let secs = (ended - started).num_seconds();
        let count = bucket_count(secs, bucket_secs);
        let mut buckets = vec![Bucket::default(); count];
        for (event, at) in events[a..=b].iter().zip(&times[a..=b]) {
            let idx = (((*at - started).num_seconds() / bucket_secs) as usize).min(count - 1);
            let bucket = &mut buckets[idx];
            bucket.records += 1;
            bucket.tool_calls += event.tool_calls;
            bucket.tool_failures += event.tool_failures;
            bucket.user_turns += event.user_turns;
            bucket.output_tokens += event.output_tokens;
        }
        spans.push(ActivitySpan {
            started,
            ended,
            secs,
            idle_before_secs: prev_end.map_or(0, |p| (started - p).num_seconds()),
            buckets,
        });
        prev_end = Some(ended);
    }

    Activity { bucket_secs, spans }
}

fn bucket_count(span_secs: i64, bucket_secs: i64) -> usize {
    (span_secs / bucket_secs) as usize + 1
}

/// The narrowest ladder rung that keeps the whole series under [`MAX_BUCKETS`].
fn choose_bucket_secs(times: &[DateTime<Utc>], ranges: &[(usize, usize)]) -> i64 {
    let fallback = BUCKET_LADDER[BUCKET_LADDER.len() - 1];
    for &width in BUCKET_LADDER {
        let total: usize = ranges
            .iter()
            .map(|&(a, b)| bucket_count((times[b] - times[a]).num_seconds(), width))
            .sum();
        if total <= MAX_BUCKETS {
            return width;
        }
    }
    fallback
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

/// A one-line label for a user turn: the leading [`PREVIEW_CHARS`] characters
/// of what the user actually typed, with injected context left out.
///
/// Returns the label and whether it was cut short.
fn preview_of(content: &Content) -> (String, bool) {
    let raw = match content {
        Content::Text(text) => text.clone(),
        Content::Blocks(blocks) => blocks
            .iter()
            .filter(|b| b.kind == "text" && !b.is_injected_context())
            .filter_map(|b| b.text.as_deref())
            .collect::<Vec<_>>()
            .join(" "),
        Content::Empty => String::new(),
    };
    let flat = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated = flat.chars().count() > PREVIEW_CHARS;
    let preview = if truncated {
        flat.chars().take(PREVIEW_CHARS).collect()
    } else {
        flat
    };
    (preview, truncated)
}

/// Turn one `AskUserQuestion` call into involvement entries, one per question.
///
/// Returns their indices so the answers can be joined on when the result
/// record arrives.
fn push_questions(
    into: &mut Vec<Involvement>,
    block: &Block,
    at: Option<DateTime<Utc>>,
) -> Vec<usize> {
    let Some(questions) = block
        .input
        .as_ref()
        .and_then(|i| i.get("questions"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    let mut indices = Vec::new();
    for q in questions {
        let Some(question) = q.get("question").and_then(Value::as_str) else {
            continue;
        };
        indices.push(into.len());
        into.push(Involvement::Question {
            at,
            header: q.get("header").and_then(Value::as_str).map(str::to_string),
            question: question.to_string(),
            options: option_labels(q),
            chosen: None,
        });
    }
    indices
}

fn option_labels(question: &Value) -> Vec<String> {
    question
        .get("options")
        .and_then(Value::as_array)
        .map(|opts| {
            opts.iter()
                .filter_map(|o| o.get("label").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Join an `AskUserQuestion` result's `answers` map onto the questions it
/// answered. The map is keyed by the question text itself.
///
/// A question with no matching answer keeps `chosen: None` — an interrupted
/// prompt is a real thing, and inventing a selection would be worse than
/// showing none.
fn fill_answers(into: &mut [Involvement], indices: &[usize], result: Option<&Value>) {
    let Some(answers) = result
        .and_then(|r| r.get("answers"))
        .and_then(Value::as_object)
    else {
        return;
    };
    for &i in indices {
        if let Some(Involvement::Question {
            question, chosen, ..
        }) = into.get_mut(i)
            && let Some(answer) = answers.get(question.as_str())
        {
            *chosen = match answer {
                Value::String(s) => Some(s.clone()),
                Value::Array(items) => Some(
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
                _ => None,
            };
        }
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
    fn the_activity_series_splits_at_idle_gaps() {
        let t = transcript(&[
            r#"{"type":"user","timestamp":"2026-08-20T10:00:00.000Z"}"#,
            r#"{"type":"assistant","timestamp":"2026-08-20T10:00:30.000Z"}"#,
            r#"{"type":"user","timestamp":"2026-08-20T12:00:30.000Z"}"#,
            r#"{"type":"assistant","timestamp":"2026-08-20T12:00:40.000Z"}"#,
        ]);
        let a = summarize(None, &t).activity;
        assert_eq!(a.spans.len(), 2);
        assert_eq!(a.spans[0].secs, 30);
        assert_eq!(a.spans[0].idle_before_secs, 0);
        assert_eq!(a.spans[1].secs, 10);
        assert_eq!(a.spans[1].idle_before_secs, 7200);
        // Every record lands in exactly one bucket, and idle occupies none.
        let counted: u32 = a
            .spans
            .iter()
            .flat_map(|s| &s.buckets)
            .map(|b| b.records)
            .sum();
        assert_eq!(counted, 4);
    }

    /// The bucket width is a fact about the session, so a long session stays
    /// renderable instead of producing thousands of columns.
    #[test]
    fn bucket_width_widens_so_the_series_stays_bounded() {
        let short = transcript(&[
            r#"{"type":"user","timestamp":"2026-08-20T10:00:00.000Z"}"#,
            r#"{"type":"user","timestamp":"2026-08-20T10:01:00.000Z"}"#,
        ]);
        assert_eq!(summarize(None, &short).activity.bucket_secs, 5);

        // Six hours of unbroken work cannot fit in five-second buckets.
        let lines: Vec<String> = (0..360)
            .map(|m| {
                format!(
                    r#"{{"type":"user","timestamp":"2026-08-20T{:02}:{:02}:00.000Z"}}"#,
                    10 + m / 60,
                    m % 60
                )
            })
            .collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let a = summarize(None, &transcript(&refs)).activity;
        assert!(a.bucket_secs > 5, "width did not widen: {}", a.bucket_secs);
        let total: usize = a.spans.iter().map(|s| s.buckets.len()).sum();
        assert!(total <= MAX_BUCKETS, "{total} buckets is too many");
    }

    #[test]
    fn prompts_carry_a_label_and_injected_context_is_left_out_of_it() {
        let long = "x".repeat(200);
        let t = transcript(&[
            &format!(r#"{{"type":"user","message":{{"content":"  fix   the\n build {long}"}}}}"#),
            r#"{"type":"user","message":{"content":[
                {"type":"text","text":"<ide_opened_file>src/lib.rs"},
                {"type":"text","text":"now ship it"}]}}"#,
        ]);
        let s = summarize(None, &t);
        assert_eq!(s.user_involvement.len(), 2);
        match &s.user_involvement[0] {
            Involvement::Prompt {
                preview, truncated, ..
            } => {
                assert!(preview.starts_with("fix the build xxx"), "{preview}");
                assert_eq!(preview.chars().count(), PREVIEW_CHARS);
                assert!(truncated);
            }
            other => panic!("expected a prompt, got {other:?}"),
        }
        match &s.user_involvement[1] {
            Involvement::Prompt {
                preview, truncated, ..
            } => {
                assert_eq!(preview, "now ship it");
                assert!(!truncated);
            }
            other => panic!("expected a prompt, got {other:?}"),
        }
    }

    #[test]
    fn questions_carry_what_was_asked_and_what_was_chosen() {
        let t = transcript(&[
            r#"{"type":"assistant","message":{"content":[
                {"type":"tool_use","id":"t1","name":"AskUserQuestion","input":{"questions":[
                    {"question":"Which store?","header":"Store","options":[
                        {"label":"Postgres"},{"label":"SQLite"}]},
                    {"question":"Ship now?","header":"Timing","options":[{"label":"Yes"}]}]}}]}}"#,
            r#"{"type":"user","message":{"content":[
                {"type":"tool_result","tool_use_id":"t1"}]},
                "toolUseResult":{"answers":{"Which store?":"Postgres"}}}"#,
        ]);
        let s = summarize(None, &t);
        assert_eq!(s.ask_user_questions, 1);
        let questions: Vec<_> = s
            .user_involvement
            .iter()
            .filter_map(|i| match i {
                Involvement::Question {
                    question,
                    options,
                    chosen,
                    ..
                } => Some((question.as_str(), options.len(), chosen.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            questions,
            vec![
                ("Which store?", 2, Some("Postgres".to_string())),
                // Unanswered stays unanswered rather than guessing.
                ("Ship now?", 1, None),
            ]
        );
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
