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

/// Share of a phase's tool calls, as a percentage, at which each label fires.
///
/// These live here rather than in the renderer for the same reason
/// [`BUCKET_LADDER`] does: two renderings of one session must not disagree
/// about what a phase was. They are integers, and the comparison is integer
/// arithmetic, so the classification cannot drift with a platform's floats.
///
/// The order they are tested in is part of the rule and lives in
/// [`classify_phase`]. Editing is deliberately the cheapest label to earn: a
/// change is almost always preceded by a lot of reading, so a phase that reads
/// twenty files and edits two is implementing, not exploring.
const IMPLEMENTING_EDIT_PCT: u32 = 15;
const FILING_ORG_PCT: u32 = 40;
const EXPLORING_READ_PCT: u32 = 50;
const RUNNING_RUN_PCT: u32 = 50;
const DELEGATING_PCT: u32 = 34;

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

/// What a tool call was *for*, coarsely enough to be stable.
///
/// The point of this classification is to survive a tool being renamed or an
/// MCP server being added: it is a small table plus one rule for MCP names,
/// not an inventory. A tool nobody has taught it about lands in `Other`, which
/// dilutes every share equally rather than distorting one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolClass {
    Read,
    Edit,
    Run,
    /// Systems of record — issue trackers, memory stores, mail. Reading and
    /// writing one are the same activity from the session's point of view:
    /// the work is filing, not coding.
    Org,
    Ask,
    Delegate,
    Other,
}

/// MCP operations that act on *files* rather than on an external system.
///
/// Matched on the operation exactly, never as a prefix, which is what keeps
/// `read` (a file server) apart from `read_report` (a tracker) — and why
/// `list_work_items` does not look like `list`.
const MCP_FILE_READ_OPS: &[&str] = &["read", "search", "list", "stat", "diff", "journal", "roots"];
const MCP_FILE_EDIT_OPS: &[&str] = &["edit", "write", "revert"];

fn classify_tool(name: &str) -> ToolClass {
    if let Some(rest) = name.strip_prefix("mcp__") {
        let op = rest.rsplit("__").next().unwrap_or(rest);
        return if MCP_FILE_READ_OPS.contains(&op) {
            ToolClass::Read
        } else if MCP_FILE_EDIT_OPS.contains(&op) {
            ToolClass::Edit
        } else {
            ToolClass::Org
        };
    }
    match name {
        "Read"
        | "Glob"
        | "Grep"
        | "LS"
        | "NotebookRead"
        | "WebFetch"
        | "WebSearch"
        | "ListMcpResourcesTool"
        | "ReadMcpResourceTool"
        | "ReadMcpResourceDirTool" => ToolClass::Read,
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => ToolClass::Edit,
        "Bash" | "PowerShell" | "BashOutput" | "KillShell" | "KillBash" | "Monitor" => {
            ToolClass::Run
        }
        "AskUserQuestion" => ToolClass::Ask,
        "Agent" | "Task" | "Skill" | "Workflow" | "SendMessage" => ToolClass::Delegate,
        _ => ToolClass::Other,
    }
}

/// The tool mix a phase's label was computed from, carried alongside the label
/// so the label can be checked rather than believed.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ToolMix {
    pub read: u32,
    pub edit: u32,
    pub run: u32,
    pub org: u32,
    pub ask: u32,
    pub delegate: u32,
    pub other: u32,
}

impl ToolMix {
    fn add(&mut self, class: ToolClass) {
        let slot = match class {
            ToolClass::Read => &mut self.read,
            ToolClass::Edit => &mut self.edit,
            ToolClass::Run => &mut self.run,
            ToolClass::Org => &mut self.org,
            ToolClass::Ask => &mut self.ask,
            ToolClass::Delegate => &mut self.delegate,
            ToolClass::Other => &mut self.other,
        };
        *slot += 1;
    }

    fn absorb(&mut self, other: &ToolMix) {
        self.read += other.read;
        self.edit += other.edit;
        self.run += other.run;
        self.org += other.org;
        self.ask += other.ask;
        self.delegate += other.delegate;
        self.other += other.other;
    }

    pub fn total(&self) -> u32 {
        self.read + self.edit + self.run + self.org + self.ask + self.delegate + self.other
    }

    /// The named parts, largest first, for a renderer that wants to show the
    /// mix without knowing the field names.
    pub fn parts(&self) -> Vec<(&'static str, u32)> {
        let mut parts = vec![
            ("read", self.read),
            ("edit", self.edit),
            ("run", self.run),
            ("org", self.org),
            ("ask", self.ask),
            ("delegate", self.delegate),
            ("other", self.other),
        ];
        parts.retain(|(_, n)| *n > 0);
        parts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        parts
    }
}

/// What a phase was, named mechanically.
///
/// These name a *tool mix*, not an intent. `implementing` means "files were
/// edited here", not "this is where the feature was built"; `running` means
/// "mostly shell", which under agent instructions that prefer shell editing
/// may well be editing kagviz cannot see. A descriptive label is a later,
/// separate field written by a model over these facts — it will never
/// overwrite one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseKind {
    Exploring,
    Implementing,
    Running,
    Filing,
    Delegating,
    Discussing,
    Mixed,
}

impl PhaseKind {
    pub fn label(self) -> &'static str {
        match self {
            PhaseKind::Exploring => "exploring",
            PhaseKind::Implementing => "implementing",
            PhaseKind::Running => "running",
            PhaseKind::Filing => "filing",
            PhaseKind::Delegating => "delegating",
            PhaseKind::Discussing => "discussing",
            PhaseKind::Mixed => "mixed",
        }
    }
}

/// One stretch of work between two user turns.
///
/// Phases are cut at every user turn **and** at every idle break, so a phase
/// never spans a gap: `span` names the [`ActivitySpan`] it lies inside, and
/// `secs` is time actually worked. Cutting only at user turns would let one
/// phase quietly contain a three-day pause and report it as duration, which is
/// the wall-clock lie one level up.
#[derive(Debug, Serialize, Deserialize)]
pub struct Phase {
    pub started: DateTime<Utc>,
    pub ended: DateTime<Utc>,
    pub secs: i64,
    /// Index into `activity.spans`. A phase lies wholly within one span.
    pub span: usize,
    pub kind: PhaseKind,
    pub records: u32,
    pub tool_calls: u32,
    pub tool_failures: u32,
    pub output_tokens: u64,
    /// The counts [`PhaseKind`] was derived from.
    pub mix: ToolMix,
    /// Preview of the user turn that opened this phase. Absent when the phase
    /// opens a resumed span instead — work picked up again with nothing said.
    pub opened_by: Option<String>,
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
    /// The session cut into phases and labelled by tool mix. Additive: it is
    /// derived from the same event stream `activity` is, and moves nothing.
    pub phases: Vec<Phase>,
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

    /// Phase kinds by time spent, largest first: `(kind, phases, secs)`.
    pub fn phase_rollup(&self) -> Vec<(PhaseKind, usize, i64)> {
        let mut by: BTreeMap<PhaseKind, (usize, i64)> = BTreeMap::new();
        for p in &self.phases {
            let slot = by.entry(p.kind).or_default();
            slot.0 += 1;
            slot.1 += p.secs;
        }
        let mut rows: Vec<(PhaseKind, usize, i64)> =
            by.into_iter().map(|(k, (n, secs))| (k, n, secs)).collect();
        rows.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.cmp(&a.1)).then(a.0.cmp(&b.0)));
        rows
    }

    /// The kind that holds the most time, for a one-word headline.
    pub fn dominant_phase(&self) -> Option<PhaseKind> {
        self.phase_rollup().first().map(|(kind, _, _)| *kind)
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
                event.mix.add(classify_tool(&name));
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
                event.user_preview = Some(preview.clone());
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
    // Spans and phases are two cuts of the same event stream, so the idle
    // split is computed once and shared: a phase boundary that disagreed with
    // a span boundary would put a phase across a gap the strip has collapsed.
    let times: Vec<DateTime<Utc>> = events.iter().filter_map(|e| e.at).collect();
    let ranges = split_spans(&times);
    s.activity = build_activity(&events, &times, &ranges);
    s.phases = build_phases(&events, &times, &ranges);

    s
}

/// One record's contribution to the activity series and the phase cut.
#[derive(Debug, Default, Clone)]
struct Event {
    at: Option<DateTime<Utc>>,
    tool_calls: u32,
    tool_failures: u32,
    user_turns: u32,
    output_tokens: u64,
    /// How this record's tool calls classified. Buckets never see it —
    /// a label in a bucket would be the wrong layer — but phases are cut and
    /// named from it.
    mix: ToolMix,
    /// Set only on a real user turn: the preview that opens a phase.
    user_preview: Option<String>,
}

/// Inclusive index ranges of continuous work, cut wherever a gap reaches
/// [`IDLE_GAP_SECS`].
///
/// The single definition of "a stretch of work": both the activity strip and
/// the phase cut are built from what this returns.
fn split_spans(times: &[DateTime<Utc>]) -> Vec<(usize, usize)> {
    if times.is_empty() {
        return Vec::new();
    }
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    for i in 1..times.len() {
        if (times[i] - times[i - 1]).num_seconds() >= IDLE_GAP_SECS {
            ranges.push((start, i - 1));
            start = i;
        }
    }
    ranges.push((start, times.len() - 1));
    ranges
}

/// Bucket each span of the event stream at a width chosen for the session.
fn build_activity(
    events: &[Event],
    times: &[DateTime<Utc>],
    ranges: &[(usize, usize)],
) -> Activity {
    if times.is_empty() {
        return Activity::default();
    }

    let bucket_secs = choose_bucket_secs(times, ranges);
    let mut spans = Vec::with_capacity(ranges.len());
    let mut prev_end: Option<DateTime<Utc>> = None;

    for &(a, b) in ranges {
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

/// Cut each span at its user turns and label the pieces by tool mix.
///
/// Two cuts, both deliberate. **User turns** because that is where the work
/// was redirected — the boundary a reader already recognises. **Span
/// boundaries** because a phase that ran across an idle gap would report a
/// three-day pause as its own duration, and `secs` here has to mean time
/// worked. So a phase that resumes after a break is a new phase, with
/// `opened_by` absent to say nobody asked for it.
///
/// A phase runs until the next one *starts*, not until its own last record.
/// The 40 seconds between an agent's last tool call and the user's next turn
/// are real work time, and giving them to neither phase would make the phase
/// durations quietly fail to add up to active time — a shortfall a reader
/// would have no way to see. Phases therefore tile their span exactly.
fn build_phases(
    events: &[Event],
    times: &[DateTime<Utc>],
    ranges: &[(usize, usize)],
) -> Vec<Phase> {
    let mut phases = Vec::new();
    for (span, &(a, b)) in ranges.iter().enumerate() {
        let mut cuts = vec![a];
        cuts.extend(((a + 1)..=b).filter(|&i| events[i].user_turns > 0));
        for (k, &start) in cuts.iter().enumerate() {
            let (last, ends_at) = match cuts.get(k + 1) {
                Some(&next) => (next - 1, times[next]),
                None => (b, times[b]),
            };
            phases.push(phase_from(
                events, times, span, times[a], start, last, ends_at,
            ));
        }
    }
    phases
}

fn phase_from(
    events: &[Event],
    times: &[DateTime<Utc>],
    span: usize,
    span_started: DateTime<Utc>,
    a: usize,
    b: usize,
    ends_at: DateTime<Utc>,
) -> Phase {
    let mut mix = ToolMix::default();
    let mut tool_calls = 0;
    let mut tool_failures = 0;
    let mut output_tokens = 0;
    for e in &events[a..=b] {
        tool_calls += e.tool_calls;
        tool_failures += e.tool_failures;
        output_tokens += e.output_tokens;
        mix.absorb(&e.mix);
    }
    let started = times[a];
    // Both ends measured as whole-second offsets from the span, then
    // subtracted, so the truncations telescope and the phases in a span sum to
    // exactly the span's own length. Truncating each phase's own duration
    // instead loses up to a second per phase — under a second each, but three
    // seconds a span and minutes across a session with two hundred of them,
    // and a shortfall that size is invisible to a reader.
    let from = (started - span_started).num_seconds();
    let to = (ends_at - span_started).num_seconds();
    Phase {
        started,
        ended: ends_at,
        secs: to - from,
        span,
        kind: classify_phase(&mix),
        records: (b - a + 1) as u32,
        tool_calls,
        tool_failures,
        output_tokens,
        mix,
        opened_by: events[a].user_preview.clone(),
    }
}

/// Name a phase from its tool mix. First rule that fires wins, and the order
/// is the rule — see the threshold constants for why editing is tested early.
///
/// Deliberately dumb. A segmentation that moves between runs is worth less
/// than a slightly coarse one that does not, and there is no way to test the
/// clever version for the property that actually matters.
fn classify_phase(mix: &ToolMix) -> PhaseKind {
    let total = mix.total();
    if total == 0 {
        return PhaseKind::Discussing;
    }
    // Integer comparison, so the label cannot depend on a platform's floats.
    let at_least = |n: u32, pct: u32| u64::from(n) * 100 >= u64::from(total) * u64::from(pct);
    if at_least(mix.delegate, DELEGATING_PCT) {
        PhaseKind::Delegating
    } else if at_least(mix.edit, IMPLEMENTING_EDIT_PCT) {
        PhaseKind::Implementing
    } else if at_least(mix.org, FILING_ORG_PCT) {
        PhaseKind::Filing
    } else if at_least(mix.read, EXPLORING_READ_PCT) {
        PhaseKind::Exploring
    } else if at_least(mix.run, RUNNING_RUN_PCT) {
        PhaseKind::Running
    } else {
        PhaseKind::Mixed
    }
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

    /// The cut rule, both halves of it. Two user turns inside one stretch of
    /// work make two phases; an idle gap makes a third, even though nobody
    /// said anything to start it.
    #[test]
    fn phases_are_cut_at_user_turns_and_never_span_an_idle_gap() {
        let t = transcript(&[
            r#"{"type":"user","timestamp":"2026-08-20T10:00:00.000Z","message":{
                "content":"explore the extractor"}}"#,
            r#"{"type":"assistant","timestamp":"2026-08-20T10:00:20.000Z","message":{
                "content":[{"type":"tool_use","id":"t1","name":"Read"}]}}"#,
            r#"{"type":"user","timestamp":"2026-08-20T10:01:00.000Z","message":{
                "content":"now fix it"}}"#,
            r#"{"type":"assistant","timestamp":"2026-08-20T10:01:30.000Z","message":{
                "content":[{"type":"tool_use","id":"t2","name":"Edit"}]}}"#,
            // two hours later: same task, but a new span and so a new phase
            r#"{"type":"assistant","timestamp":"2026-08-20T12:01:30.000Z","message":{
                "content":[{"type":"tool_use","id":"t3","name":"Bash"}]}}"#,
        ]);
        let s = summarize(None, &t);
        assert_eq!(s.phases.len(), 3);
        assert_eq!(s.activity.spans.len(), 2);

        assert_eq!(s.phases[0].span, 0);
        assert_eq!(
            s.phases[0].opened_by.as_deref(),
            Some("explore the extractor")
        );
        assert_eq!(s.phases[0].kind, PhaseKind::Exploring);

        assert_eq!(s.phases[1].span, 0);
        assert_eq!(s.phases[1].opened_by.as_deref(), Some("now fix it"));
        assert_eq!(s.phases[1].kind, PhaseKind::Implementing);

        // The resumed phase: nobody opened it, and its duration is work only.
        assert_eq!(s.phases[2].span, 1);
        assert_eq!(s.phases[2].opened_by, None);
        assert_eq!(s.phases[2].secs, 0);

        // No phase swallowed the two-hour gap, and between them the phases
        // account for every second of active time — no unattributed remainder.
        assert!(s.phases.iter().all(|p| p.secs < IDLE_GAP_SECS));
        assert_eq!(
            s.phases.iter().map(|p| p.secs).sum::<i64>(),
            s.activity.spans.iter().map(|sp| sp.secs).sum::<i64>()
        );
        // The first phase runs up to the moment the user redirected it, not
        // up to its own last tool call.
        assert_eq!(s.phases[0].secs, 60);
        assert_eq!(s.phases[0].ended, s.phases[1].started);
    }

    /// Real timestamps carry milliseconds, and truncating each phase's own
    /// duration lost up to a second per phase — three seconds a span, and
    /// minutes across the 209-span session in the corpus. Found by the sweep,
    /// not by a unit test, which is why this one exists.
    #[test]
    fn sub_second_timestamps_do_not_leak_time_out_of_a_span() {
        let t = transcript(&[
            r#"{"type":"user","timestamp":"2026-08-20T10:00:00.900Z","message":{
                "content":"a"}}"#,
            r#"{"type":"user","timestamp":"2026-08-20T10:00:10.800Z","message":{
                "content":"b"}}"#,
            r#"{"type":"user","timestamp":"2026-08-20T10:00:20.700Z","message":{
                "content":"c"}}"#,
            r#"{"type":"user","timestamp":"2026-08-20T10:00:30.600Z","message":{
                "content":"d"}}"#,
        ]);
        let s = summarize(None, &t);
        assert_eq!(s.phases.len(), 4);
        assert_eq!(s.activity.spans.len(), 1);
        assert_eq!(
            s.phases.iter().map(|p| p.secs).sum::<i64>(),
            s.activity.spans[0].secs,
            "phases must tile their span exactly, milliseconds and all"
        );
    }

    /// Editing is a strong signal at a low share: a phase that reads a lot and
    /// edits twice is implementing, because that is what implementing looks
    /// like. Reading with no edit at all is exploring.
    #[test]
    fn a_phase_is_named_by_its_tool_mix_not_by_its_biggest_count() {
        let reads = |n: usize| {
            (0..n)
                .map(|_| {
                    r#"{"type":"assistant","timestamp":"2026-08-20T10:00:20.000Z","message":{
                        "content":[{"type":"tool_use","name":"Read"}]}}"#
                })
                .collect::<Vec<_>>()
        };

        let mut lines = reads(9);
        let s = summarize(None, &transcript(&lines));
        assert_eq!(s.phases[0].kind, PhaseKind::Exploring);
        assert_eq!(s.phases[0].mix.read, 9);

        let edit = r#"{"type":"assistant","timestamp":"2026-08-20T10:00:20.000Z","message":{
                "content":[{"type":"tool_use","name":"Edit"}]}}"#;

        lines.push(edit);
        let s = summarize(None, &transcript(&lines));
        assert_eq!(s.phases[0].mix.edit, 1);
        assert_eq!(
            s.phases[0].kind,
            PhaseKind::Exploring,
            "1 edit in 10 is 10%, under the 15% bar"
        );

        lines.push(edit);
        let s = summarize(None, &transcript(&lines));
        assert_eq!(
            s.phases[0].kind,
            PhaseKind::Implementing,
            "2 edits in 11 is 18%, over it"
        );
    }

    /// A phase with no tool calls at all is the agent talking, and saying so
    /// is better than calling it `mixed`.
    #[test]
    fn a_phase_with_no_tool_calls_is_discussing() {
        let t = transcript(&[
            r#"{"type":"user","timestamp":"2026-08-20T10:00:00.000Z","message":{
                "content":"what does this do?"}}"#,
            r#"{"type":"assistant","timestamp":"2026-08-20T10:00:20.000Z","message":{
                "content":[{"type":"text","text":"it counts records"}]}}"#,
        ]);
        let s = summarize(None, &t);
        assert_eq!(s.phases.len(), 1);
        assert_eq!(s.phases[0].kind, PhaseKind::Discussing);
        assert_eq!(s.phases[0].mix.total(), 0);
    }

    /// MCP tools are classified by their *operation*, so a file server and a
    /// tracker on the same protocol do not read as the same activity. The
    /// match is exact: `list_work_items` is not `list`.
    #[test]
    fn mcp_tools_are_classified_by_operation_not_by_server() {
        assert_eq!(classify_tool("mcp__kaed-kai__read"), ToolClass::Read);
        assert_eq!(classify_tool("mcp__kaed-kai__edit"), ToolClass::Edit);
        assert_eq!(classify_tool("mcp__korg__list_work_items"), ToolClass::Org);
        assert_eq!(classify_tool("mcp__korg__create_work_item"), ToolClass::Org);
        assert_eq!(classify_tool("mcp__klams__memory_search"), ToolClass::Org);
        // Nothing unknown is silently a read; it dilutes, it does not distort.
        assert_eq!(classify_tool("SomeToolNobodyTaughtUs"), ToolClass::Other);
    }

    #[test]
    fn a_tracker_heavy_phase_is_filing() {
        let t = transcript(&[
            r#"{"type":"user","timestamp":"2026-08-20T10:00:00.000Z","message":{
                "content":"file the follow-ups"}}"#,
            r#"{"type":"assistant","timestamp":"2026-08-20T10:00:20.000Z","message":{
                "content":[
                    {"type":"tool_use","name":"mcp__korg__create_work_item"},
                    {"type":"tool_use","name":"mcp__korg__create_work_item"},
                    {"type":"tool_use","name":"mcp__korg__update_proposal"},
                    {"type":"tool_use","name":"Read"}]}}"#,
        ]);
        let s = summarize(None, &t);
        assert_eq!(s.phases[0].kind, PhaseKind::Filing);
        assert_eq!(s.phases[0].mix.org, 3);
        assert_eq!(s.phases[0].mix.read, 1);
    }

    /// The rollup is what the headline reads, so it has to order by time
    /// spent rather than by phase count.
    #[test]
    fn the_phase_rollup_orders_by_time_not_by_count() {
        let t = transcript(&[
            r#"{"type":"user","timestamp":"2026-08-20T10:00:00.000Z","message":{
                "content":"a"}}"#,
            r#"{"type":"assistant","timestamp":"2026-08-20T10:01:00.000Z","message":{
                "content":[{"type":"tool_use","name":"Edit"}]}}"#,
            r#"{"type":"user","timestamp":"2026-08-20T10:01:10.000Z","message":{
                "content":"b"}}"#,
            r#"{"type":"assistant","timestamp":"2026-08-20T10:01:15.000Z","message":{
                "content":[{"type":"tool_use","name":"Read"}]}}"#,
            r#"{"type":"user","timestamp":"2026-08-20T10:01:20.000Z","message":{
                "content":"c"}}"#,
            r#"{"type":"assistant","timestamp":"2026-08-20T10:01:25.000Z","message":{
                "content":[{"type":"tool_use","name":"Read"}]}}"#,
        ]);
        let s = summarize(None, &t);
        let rollup = s.phase_rollup();
        // one implementing phase (70s, up to the next user turn) against two
        // exploring phases (10s and 5s)
        assert_eq!(rollup[0], (PhaseKind::Implementing, 1, 70));
        assert_eq!(rollup[1], (PhaseKind::Exploring, 2, 15));
        assert_eq!(
            rollup.iter().map(|(_, _, secs)| secs).sum::<i64>(),
            s.activity.spans.iter().map(|sp| sp.secs).sum::<i64>()
        );
        assert_eq!(s.dominant_phase(), Some(PhaseKind::Implementing));
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
