//! The deterministic pass: transcript records in, counted facts out.
//!
//! Everything here is a pure function of the bytes on disk. The same
//! transcript yields the same summary forever, which is the whole point — any
//! model-written narrative sits *on top* of this, never inside it.

use crate::discover::SessionPaths;
use crate::transcript::{
    Block, Content, INJECTED_PREFIXES, Record, Subagent, Transcript, command_line,
};
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
///
/// Every quantity here is in exactly one of two states, and the document says
/// which: **recovered** (an exact number, read out of a diff the transcript
/// carries) or **unrecovered** (a call that could have changed a file and
/// exposed nothing readable). There is no third state — an *inferred* number,
/// such as a `git diff` over the session window, is not a function of the
/// transcript bytes and does not belong in this document. See
/// `docs/facts-contract.md`.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct FileChanges {
    pub files_touched: usize,
    pub lines_added: usize,
    pub lines_deleted: usize,
    /// Tool calls that plainly could have changed files but exposed no
    /// recoverable diff.
    ///
    /// `Bash` heredocs and `sed` edits land here, and so does any file-editing
    /// MCP tool whose result [`recover_changes`] could not read. Surfaced
    /// rather than hidden, because a zero that means "nothing changed" and a
    /// zero that means "kagviz could not see it" are very different readings.
    pub opaque_edits: usize,
    /// The same picture broken out per tool: which tools kagviz recovered
    /// exact numbers from, and which ones it could only count.
    ///
    /// This is the audit surface for the adapter table — the same argument
    /// [`ToolMix`] makes for a phase's [`PhaseKind`]. Without it a reader has
    /// to take "+340 −88, and 51 unseen" on faith; with it they can see that
    /// the 51 were `Bash` and the 340 came from `Edit` and `mcp__kaed-kai__edit`.
    pub by_tool: BTreeMap<String, ToolChanges>,
}

/// One tool's contribution to the file-change picture.
///
/// `calls` is every edit-capable call of the tool. `opaque` counts those whose
/// **line deltas** could not be read — which is not quite the same as learning
/// nothing from them: a call that named its files but carried no diff is
/// opaque here and still counted in `files_touched`. So `files_touched` can be
/// exact on a tool whose `opaque` is non-zero, and the two must be read
/// separately rather than as one verdict on the call.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ToolChanges {
    /// Calls of this tool that could have changed a file.
    pub calls: usize,
    pub files_touched: usize,
    pub lines_added: usize,
    pub lines_deleted: usize,
    /// Of `calls`, those that exposed no diff kagviz could read.
    pub opaque: usize,
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

/// One delegated agent's work.
///
/// A subagent transcript is a session transcript in miniature, but it is not
/// summarized as one: it has no user turns to cut phases at, and its activity
/// runs *concurrently* with the parent's, so it has no place on the parent's
/// time strip. What it has is cost and output, and that is what is carried.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Spawn {
    /// Joins the sidecar file, its records, and the parent's `Agent` result.
    pub agent_id: Option<String>,
    /// From the parent's `Agent` call input, when the spawn could be joined.
    pub subagent_type: Option<String>,
    /// From the parent's `Agent` result — what the agent was asked for.
    pub description: Option<String>,
    /// `resolvedModel` from the parent's `Agent` result. A delegated turn can
    /// run on a different model than the session it was spawned from, which is
    /// exactly the kind of cost a reader wants to see.
    pub model: Option<String>,
    /// True when the numbers came from a `subagents/agent-*.jsonl` sidecar,
    /// false when they came from `isSidechain` records inlined in the parent
    /// by an older CLI.
    pub sidecar: bool,
    pub started: Option<DateTime<Utc>>,
    pub ended: Option<DateTime<Utc>>,
    /// Wall span less idle gaps, by the same [`IDLE_GAP_SECS`] rule the parent
    /// uses. **Not** addable to the parent's `active_secs`: a subagent runs
    /// while the parent waits, so the two overlap in real time.
    pub active_secs: i64,
    pub records: usize,
    pub skipped_lines: usize,
    pub assistant_turns: usize,
    pub tool_calls: BTreeMap<String, u32>,
    pub tool_failures: BTreeMap<String, u32>,
    pub tokens: TokenTotals,
    pub changes: FileChanges,
}

/// Everything the delegated tier adds up to.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DelegatedTotals {
    pub records: usize,
    pub assistant_turns: usize,
    pub tool_calls: BTreeMap<String, u32>,
    pub tool_failures: BTreeMap<String, u32>,
    pub tokens: TokenTotals,
    pub changes: FileChanges,
}

/// Work the session handed to subagents, reported as its own tier.
///
/// Deliberately **not** folded into the parent's totals. Burying delegated
/// cost inside the parent hides the number a reader most wants to see — one
/// corpus spawn ran 48 tool calls and 25k output tokens behind a single
/// `Agent` call in the parent. So the parent's numbers stay exactly what they
/// always were, this tier stands beside them, and the report draws an explicit
/// combined line from [`Summary::combined_tool_calls`] and its siblings.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Delegation {
    pub spawns: Vec<Spawn>,
    /// `Agent` calls in the parent with no transcript to join to. The work
    /// happened and kagviz cannot see it — an unknown, reported rather than
    /// left to look like nothing.
    pub unjoined_spawns: usize,
    /// Records lifted out of the parent's counts because they were subagent
    /// turns an older CLI inlined (`isSidechain`). Reported so the move is
    /// visible rather than silent.
    pub inline_records: usize,
    pub totals: DelegatedTotals,
}

impl Delegation {
    pub fn is_empty(&self) -> bool {
        self.spawns.is_empty() && self.unjoined_spawns == 0
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
    /// Work handed to subagents. A separate tier, never merged into the
    /// numbers above — see [`Delegation`].
    pub delegation: Delegation,

    /// Prose a model wrote over everything above — a headline and a label per
    /// phase. Absent unless `--label` produced it or the facts document
    /// carried it, and never in the path that produces a number: see
    /// [`crate::label`] for why the phase labels are a parallel array rather
    /// than a field on [`Phase`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<crate::label::Labels>,
}

impl Summary {
    pub fn total_tool_calls(&self) -> u32 {
        self.tool_calls.values().sum()
    }

    pub fn total_tool_failures(&self) -> u32 {
        self.tool_failures.values().sum()
    }

    /// The share of this session's tool calls that failed, as a ratio.
    ///
    /// Over the calls, not calls plus failures: `tool_calls` counts every
    /// `tool_use` and `tool_failures` counts the results that came back
    /// `is_error`, joined to those same calls — a failed call is a call,
    /// already counted once, and adding it again undercounts the rate.
    ///
    /// `None` when there is nothing worth dividing: no calls, or no failure
    /// that joined to one. The zero case reads as `none failed` wherever it
    /// is shown, and a `0.00%` beside that is noise. Failures blamed on
    /// `<unknown>` are left out of the numerator — their calls are not in the
    /// file, so they are not in the denominator either, and counting them
    /// would let the rate pass 100% on a session whose one visible call
    /// succeeded. The tool mix says how many there were. (Measured across the
    /// corpus: no session has any, so this is a guard, not a case.)
    ///
    /// A method, not a field, for the reason `combined_tool_calls` is: the
    /// facts carry the two counts once, and a quotient anyone can recompute
    /// is not a separate fact.
    pub fn tool_failure_rate(&self) -> Option<f64> {
        let calls = self.total_tool_calls();
        let unknown = self.tool_failures.get("<unknown>").copied().unwrap_or(0);
        let joined = self.total_tool_failures().saturating_sub(unknown);
        (calls > 0 && joined > 0).then(|| f64::from(joined) / f64::from(calls))
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

    /// Tool calls by this session *and* everything it delegated.
    ///
    /// The combined line is a method rather than a serialized field, following
    /// `total_tool_calls`: the facts carry each tier once, and a sum anyone can
    /// recompute is not a separate fact. What is not optional is *showing* it —
    /// making a reader add two numbers to learn what the session cost is the
    /// same failure as hiding one of them.
    pub fn combined_tool_calls(&self) -> u32 {
        self.total_tool_calls() + self.delegation.totals.tool_calls.values().sum::<u32>()
    }

    pub fn combined_tool_failures(&self) -> u32 {
        self.total_tool_failures() + self.delegation.totals.tool_failures.values().sum::<u32>()
    }

    /// Output tokens across both tiers. Tokens add across concurrent agents
    /// where seconds do not, which is why there is no combined active time.
    pub fn combined_output_tokens(&self) -> u64 {
        self.tokens.output + self.delegation.totals.tokens.output
    }
}

/// Tools that routinely change files without exposing a diff kagviz can read.
fn may_edit_opaquely(tool: &str) -> bool {
    matches!(tool, "Bash" | "PowerShell")
}

/// Count a session.
///
/// `subagents` are the session's already-read `subagents/agent-*.jsonl`
/// sidecars. They are passed in rather than read here so this stays a pure
/// function of bytes the caller supplies — the property the whole facts
/// contract rests on. Callers with none pass `&[]`.
pub fn summarize(
    paths: Option<&SessionPaths>,
    transcript: &Transcript,
    subagents: &[Subagent],
) -> Summary {
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
    let mut tally = ChangeTally::default();
    // AskUserQuestion tool_use id -> the involvement entries it produced, so
    // the answers on its result can be filled in when they arrive.
    let mut open_questions: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    // `Agent` tool_use id -> the `subagent_type` it was asked for, so a spawn's
    // own transcript can be joined back to what the parent wanted from it.
    let mut agent_calls: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut agent_call_total = 0usize;
    // agentId -> what the parent's `Agent` result says about that spawn.
    let mut spawn_meta: BTreeMap<String, SpawnMeta> = BTreeMap::new();
    // Subagent turns an older CLI inlined into this file, grouped by agent.
    let mut inline: BTreeMap<String, Vec<&Record>> = BTreeMap::new();

    for rec in records {
        // A sidechain record is a subagent's turn, not the parent's. It is
        // lifted out here — before it can reach a count, an event or a phase —
        // and reported in the delegated tier instead. Newer CLI versions write
        // sidecar files and never take this branch.
        if rec.is_sidechain.unwrap_or(false) {
            inline
                .entry(rec.agent_id.clone().unwrap_or_default())
                .or_default()
                .push(rec);
            continue;
        }
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
                        agent_call_total += 1;
                        if let Some(id) = &block.id {
                            let kind = block
                                .input
                                .as_ref()
                                .and_then(|i| i.get("subagent_type"))
                                .and_then(Value::as_str)
                                .map(str::to_string);
                            agent_calls.insert(id.clone(), kind);
                        }
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
            if is_user_turn(rec, &msg.content) {
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
                // An `Agent` result names the spawn it launched. That name is
                // the only thing joining a sidecar transcript to what the
                // parent asked for, and to which model actually ran it.
                if let Some(id) = &block.tool_use_id
                    && let Some(kind) = agent_calls.remove(id)
                    && let Some(result) = &rec.tool_use_result
                    && let Some(agent_id) = result.get("agentId").and_then(Value::as_str)
                {
                    spawn_meta.insert(
                        agent_id.to_string(),
                        SpawnMeta {
                            subagent_type: kind,
                            description: result
                                .get("description")
                                .and_then(Value::as_str)
                                .map(str::to_string),
                            model: result
                                .get("resolvedModel")
                                .and_then(Value::as_str)
                                .map(str::to_string),
                        },
                    );
                }
            }
        }

        if let Some(result) = &rec.tool_use_result {
            // The tally is keyed by tool, and the name lives on the *call*, so
            // it is joined through `tool_use_id` exactly as failures are. A
            // result whose call is not in this file reads as `<unknown>`,
            // which still gets the default adapter — the same reading it got
            // before there was a table.
            let (tool, failed) = result_call(rec, &tool_names).unwrap_or(("<unknown>", false));
            tally.absorb(tool, result, failed);
        }

        if event.at.is_some() {
            events.push(event);
        }
    }

    // Shell tools that ran at all. Counted as "could have changed files
    // unseen", never as changes. Every one of them is opaque by construction,
    // so they are added from the call tally rather than from results — an
    // interrupted `Bash` leaves no result and is still an edit kagviz cannot
    // see.
    for (name, n) in &s.tool_calls {
        if may_edit_opaquely(name) {
            tally.opaque(name, *n as usize);
        }
    }
    s.changes = tally.finish();

    s.skills.sort();
    s.skills.dedup();
    s.subagents.sort();

    stamps.sort_unstable();
    if let (Some(first), Some(last)) = (stamps.first(), stamps.last()) {
        s.started = Some(*first);
        s.ended = Some(*last);
        (s.wall_secs, s.idle_secs) = wall_and_idle(&stamps);
    }

    s.delegation = build_delegation(subagents, &inline, &spawn_meta, agent_call_total);

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
    // Read back off the spans rather than recomputed, so `active_secs` is the
    // sum of the stretches *by construction*. `wall_secs - idle_secs` is the
    // same quantity through two truncations against the spans' one each, and
    // the two drifted apart — 198s of a 12h39m session with 209 spans. Phases
    // already tile their span exactly, so this makes the headline, the strip
    // and the phase list one number instead of three that nearly agree.
    s.active_secs = s.activity.spans.iter().map(|sp| sp.secs).sum();

    s
}

/// What the parent's `Agent` call and result say about one spawn.
#[derive(Debug, Default, Clone)]
struct SpawnMeta {
    subagent_type: Option<String>,
    description: Option<String>,
    model: Option<String>,
}

/// The tool a result record belongs to, and whether that call errored.
///
/// The name lives on the *call*, so it is joined through `tool_use_id` exactly
/// as a failure is. A result whose call is not in this file has no name here,
/// and the caller decides what to do about that.
fn result_call<'a>(rec: &Record, names: &'a BTreeMap<String, String>) -> Option<(&'a str, bool)> {
    rec.message
        .as_ref()
        .map(|m| m.content.blocks())
        .unwrap_or_default()
        .iter()
        .filter(|b| b.kind == "tool_result")
        .find_map(|b| {
            let name = b.tool_use_id.as_ref().and_then(|id| names.get(id))?;
            Some((name.as_str(), b.is_error.unwrap_or(false)))
        })
}

/// Wall span and idle seconds over *sorted* timestamps, by [`IDLE_GAP_SECS`].
///
/// Shared by the session and by each spawn so "active" means one thing in the
/// document. A subagent left waiting on a slow tool is idle for the same
/// reason a session left waiting on a user is.
fn wall_and_idle(stamps: &[DateTime<Utc>]) -> (i64, i64) {
    let (Some(first), Some(last)) = (stamps.first(), stamps.last()) else {
        return (0, 0);
    };
    let wall = (*last - *first).num_seconds();
    let idle = stamps
        .windows(2)
        .map(|w| (w[1] - w[0]).num_seconds())
        .filter(|gap| *gap >= IDLE_GAP_SECS)
        .sum();
    (wall, idle)
}

/// Active seconds over *sorted* timestamps: the continuous stretches of work,
/// each measured once and summed.
///
/// The same definition [`build_activity`] gives a span, applied where there
/// are no spans to read it off — the delegated tier carries no strip. Keeping
/// one definition is the point: a subagent's active time and the session's
/// have to mean the same thing to be worth printing side by side.
fn active_from_stretches(times: &[DateTime<Utc>]) -> i64 {
    split_spans(times)
        .iter()
        .map(|&(a, b)| (times[b] - times[a]).num_seconds())
        .sum()
}

/// Assemble the delegated tier from both shapes a spawn can arrive in.
///
/// Sidecar files are the current format; `inline` holds the groups lifted out
/// of the parent for the older `isSidechain` one. Both produce the same
/// [`Spawn`], so nothing downstream has to know which era a transcript is from.
fn build_delegation(
    subagents: &[Subagent],
    inline: &BTreeMap<String, Vec<&Record>>,
    meta: &BTreeMap<String, SpawnMeta>,
    agent_call_total: usize,
) -> Delegation {
    let mut d = Delegation::default();

    let mut tier = ChangeTally::default();

    for sub in subagents {
        let records: Vec<&Record> = sub.transcript.records.iter().collect();
        let (mut spawn, tally) = summarize_spawn(&records, sub.transcript.skipped, true);
        if spawn.agent_id.is_none() {
            spawn.agent_id.clone_from(&sub.agent_id);
        }
        tier.merge(&tally);
        d.spawns.push(spawn);
    }
    for (agent_id, records) in inline {
        d.inline_records += records.len();
        let (mut spawn, tally) = summarize_spawn(records, 0, false);
        if spawn.agent_id.is_none() && !agent_id.is_empty() {
            spawn.agent_id = Some(agent_id.clone());
        }
        tier.merge(&tally);
        d.spawns.push(spawn);
    }

    for spawn in &mut d.spawns {
        if let Some(m) = spawn.agent_id.as_deref().and_then(|id| meta.get(id)) {
            spawn.subagent_type.clone_from(&m.subagent_type);
            spawn.description.clone_from(&m.description);
            spawn.model.clone_from(&m.model);
        }
    }
    // Stable order regardless of how the filesystem enumerated the sidecars.
    d.spawns
        .sort_by(|a, b| (&a.started, &a.agent_id).cmp(&(&b.started, &b.agent_id)));

    // Every `Agent` call the parent made that no transcript accounts for. The
    // work happened; kagviz cannot see it. Saying nothing would render it as
    // no cost at all, which is the exact failure this document exists to avoid.
    d.unjoined_spawns = agent_call_total.saturating_sub(d.spawns.len());

    let t = &mut d.totals;
    for spawn in &d.spawns {
        t.records += spawn.records;
        t.assistant_turns += spawn.assistant_turns;
        for (name, n) in &spawn.tool_calls {
            *t.tool_calls.entry(name.clone()).or_default() += n;
        }
        for (name, n) in &spawn.tool_failures {
            *t.tool_failures.entry(name.clone()).or_default() += n;
        }
        t.tokens.input += spawn.tokens.input;
        t.tokens.output += spawn.tokens.output;
        t.tokens.thinking += spawn.tokens.thinking;
        t.tokens.cache_read += spawn.tokens.cache_read;
        t.tokens.cache_write += spawn.tokens.cache_write;
    }
    t.changes = tier.finish();
    d
}

/// Count one delegated agent's records.
///
/// A compact pass rather than a recursive [`summarize`]: a subagent has no user
/// turns, so phases, the activity strip and user involvement are all vacuous
/// for it, and carrying them would multiply the size of the facts document for
/// no reader. What a spawn has is cost and output, and that is what is kept.
fn summarize_spawn(records: &[&Record], skipped: usize, sidecar: bool) -> (Spawn, ChangeTally) {
    let mut spawn = Spawn {
        sidecar,
        records: records.len(),
        skipped_lines: skipped,
        ..Spawn::default()
    };
    let mut tool_names: BTreeMap<String, String> = BTreeMap::new();
    let mut tally = ChangeTally::default();
    let mut stamps: Vec<DateTime<Utc>> = Vec::new();

    for rec in records {
        if spawn.agent_id.is_none() {
            spawn.agent_id.clone_from(&rec.agent_id);
        }
        if let Some(at) = rec.timestamp.as_deref().and_then(parse_stamp) {
            stamps.push(at);
        }

        if rec.kind == "assistant"
            && let Some(msg) = &rec.message
        {
            spawn.assistant_turns += 1;
            if let Some(u) = msg.usage {
                spawn.tokens.input += u.input_tokens;
                spawn.tokens.output += u.output_tokens;
                spawn.tokens.thinking += u.output_tokens_details.thinking_tokens;
                spawn.tokens.cache_read += u.cache_read_input_tokens;
                spawn.tokens.cache_write += u.cache_creation_input_tokens;
            }
            for block in msg.content.blocks() {
                if block.kind != "tool_use" {
                    continue;
                }
                let Some(name) = block.name.clone() else {
                    continue;
                };
                *spawn.tool_calls.entry(name.clone()).or_default() += 1;
                if let Some(id) = &block.id {
                    tool_names.insert(id.clone(), name);
                }
            }
        }

        if rec.kind == "user"
            && let Some(msg) = &rec.message
        {
            for block in msg.content.blocks() {
                if block.kind == "tool_result" && block.is_error.unwrap_or(false) {
                    let name = block
                        .tool_use_id
                        .as_ref()
                        .and_then(|id| tool_names.get(id))
                        .cloned()
                        .unwrap_or_else(|| "<unknown>".to_string());
                    *spawn.tool_failures.entry(name).or_default() += 1;
                }
            }
        }

        if let Some(result) = &rec.tool_use_result {
            let (tool, failed) = result_call(rec, &tool_names).unwrap_or(("<unknown>", false));
            tally.absorb(tool, result, failed);
        }
    }

    for (name, n) in &spawn.tool_calls {
        if may_edit_opaquely(name) {
            tally.opaque(name, *n as usize);
        }
    }
    spawn.changes = tally.finish();

    stamps.sort_unstable();
    if let (Some(first), Some(last)) = (stamps.first(), stamps.last()) {
        spawn.started = Some(*first);
        spawn.ended = Some(*last);
        spawn.active_secs = active_from_stretches(&stamps);
    }
    (spawn, tally)
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
/// and text the harness injects (IDE state, instruction documents, system
/// reminders). Only the first is user involvement, and telling them apart is
/// the whole job — `promptId` is present on all three.
///
/// Asked in two steps, cheapest and most reliable first. The harness *flags*
/// what it wrote (`isMeta`), so that answers the question outright for a skill
/// body, a resume nudge or an attachment placeholder — no shape-matching
/// involved. Only an unflagged record reaches the content rules, and there a
/// slash command's `<command-*>` scaffold counts: it is 158 bytes of what the
/// user typed, not an injection.
fn is_user_turn(rec: &Record, content: &Content) -> bool {
    if rec.is_harness_written() {
        return false;
    }
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
/// A slash command is shown as the line the user typed — `/start-sprint
/// korg:1606 proceed with implementation` — rebuilt from the scaffold's tags
/// by [`command_line`]. Showing the tags themselves would spend the whole
/// preview on markup, and showing the instruction document that follows would
/// be showing the harness talking to the agent.
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
    let raw = command_line(&raw).unwrap_or(raw);
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

/// A file change read out of one tool result.
///
/// The two halves are recovered *independently*, because the corpus contains
/// results that carry one without the other: a file server can report
/// `{"applied":true,"files":[…]}` with no `diff` at all, naming exactly which
/// files it changed while saying nothing about how much. Collapsing that to
/// "unreadable" throws away a fact the transcript actually holds.
#[derive(Debug, Default)]
struct Recovered {
    /// Files the tool said it changed, as *it* identified them. Absolute for
    /// the built-in editors; usually root-relative for an MCP file server,
    /// which may not even be describing a file on this host.
    files: Vec<String>,
    added: usize,
    deleted: usize,
    /// Whether `added`/`deleted` are a reading or merely a default.
    ///
    /// `false` means the tool named its files but handed over no diff: the
    /// files are exact and the line counts are unknown, so the call still owes
    /// `opaque_edits` an entry even though it contributed to `files_touched`.
    lines_known: bool,
}

/// The adapter table: which reader to try against a tool's result payload.
///
/// Keyed by tool name rather than written as one branching code path, because
/// the shapes differ genuinely — `Edit` hands over parsed hunks, a file-serving
/// MCP tool hands over a unified diff inside a JSON envelope. A tool with no
/// adapter is not a bug: it is an honest `opaque_edits`.
///
/// Returning `Some(Recovered::default())` is meaningful and distinct from
/// `None`. It says "this call was read, and it changed nothing" — a refused
/// edit is a known zero, not an unknown.
fn recover_changes(tool: &str, result: &Value) -> Option<Recovered> {
    if is_mcp_file_edit(tool) {
        from_diff_envelope(result)
    } else {
        from_structured_patch(result)
    }
}

/// Tools that change files, and therefore owe the document a number.
///
/// A result kagviz cannot read from one of these is an *unknown*, not a zero —
/// so it lands in `opaque_edits`. Today every unreadable `Edit` result in the
/// corpus is a failed one (which changed nothing, and is already visible in
/// `tool_failures`), so this guard costs nothing now. It exists because the
/// format drifts: the day `Edit` grows a result shape kagviz has not been
/// taught, the number must go visibly missing rather than quietly to zero.
fn edits_files(tool: &str) -> bool {
    matches!(tool, "Edit" | "MultiEdit" | "Write" | "NotebookEdit") || is_mcp_file_edit(tool)
}

/// An MCP tool whose operation edits files, by the same exact-match rule
/// [`classify_tool`] uses. A new file server gets an adapter for free; a
/// tracker with an `update_*` operation does not get mistaken for one.
fn is_mcp_file_edit(tool: &str) -> bool {
    tool.strip_prefix("mcp__").is_some_and(|rest| {
        let op = rest.rsplit("__").next().unwrap_or(rest);
        MCP_FILE_EDIT_OPS.contains(&op)
    })
}

/// `Edit`, `Write`, `NotebookEdit`: real unified-diff hunks, already parsed.
///
/// A `create` result has an empty patch and the whole file body in `content`,
/// so its line count is the addition.
fn from_structured_patch(result: &Value) -> Option<Recovered> {
    let patch = result.get("structuredPatch")?;
    let mut out = Recovered {
        lines_known: true,
        ..Recovered::default()
    };
    if let Some(path) = result.get("filePath").and_then(Value::as_str) {
        out.files.push(path.to_string());
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
                    Some(b'+') => out.added += 1,
                    Some(b'-') => out.deleted += 1,
                    _ => {}
                }
            }
        }
    }

    if !saw_hunk
        && result.get("type").and_then(Value::as_str) == Some("create")
        && let Some(body) = result.get("content").and_then(Value::as_str)
    {
        out.added = body.lines().count();
    }
    Some(out)
}

/// A file-serving MCP tool that returns its own unified diff.
///
/// Measured shape (`mcp__kaed-*__edit`): `toolUseResult` is a JSON **string**,
/// not an object, holding
/// `{"applied":true,"diff":"--- a/x\n+++ b/x\n@@ …","files":[{"path":"x"}]}`.
/// A bare object is accepted too, so a server that returns structured content
/// reads the same way.
fn from_diff_envelope(result: &Value) -> Option<Recovered> {
    let parsed;
    let envelope = match result {
        Value::String(raw) => {
            parsed = serde_json::from_str::<Value>(raw).ok()?;
            &parsed
        }
        other => other,
    };

    // An edit the server refused changed nothing, and kagviz knows that
    // exactly. Reporting it as opaque would manufacture an unknown.
    if envelope.get("applied").and_then(Value::as_bool) == Some(false) {
        return Some(Recovered {
            lines_known: true,
            ..Recovered::default()
        });
    }

    let files: Vec<String> = envelope
        .get("files")
        .and_then(Value::as_array)
        .map(|fs| {
            fs.iter()
                .filter_map(|f| f.get("path").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    // Measured on the cleo corpus: 16 results are `{"applied":true,"files":[…]}`
    // with no `diff` — the edit landed and named its files, and only the line
    // counts are missing. Take the files; leave the lines visibly unknown.
    let Some(diff) = envelope.get("diff").and_then(Value::as_str) else {
        return (!files.is_empty()).then(|| Recovered {
            files,
            ..Recovered::default()
        });
    };
    let (added, deleted) = count_unified(diff);
    Some(Recovered {
        files,
        added,
        deleted,
        lines_known: true,
    })
}

/// Count added and deleted lines in a unified diff.
///
/// Only lines *inside* a hunk are counted, which is what keeps a deleted line
/// whose own content is `--` from reading as a `---` file header. The header
/// pair is matched as a pair, and as a path (`--- a/x`, so byte 3 is a space)
/// rather than as a prefix, for the same reason: markdown is full of `---`.
fn count_unified(diff: &str) -> (usize, usize) {
    let is_header = |line: &str, marker: u8| {
        line.len() > 4 && line.as_bytes()[..3] == [marker; 3] && line.as_bytes()[3] == b' '
    };
    let (mut added, mut deleted) = (0, 0);
    let mut in_hunk = false;
    let mut lines = diff.lines().peekable();
    while let Some(line) = lines.next() {
        if is_header(line, b'-') && lines.peek().is_some_and(|n| is_header(n, b'+')) {
            lines.next();
            in_hunk = false;
            continue;
        }
        if line.starts_with("@@") {
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            continue;
        }
        match line.as_bytes().first() {
            Some(b'+') => added += 1,
            Some(b'-') => deleted += 1,
            _ => {}
        }
    }
    (added, deleted)
}

/// Accumulates the file-change picture over a pass of records.
///
/// Holds the per-tool path sets that `FileChanges` only carries counts of, so
/// the same tool editing one file twice is one file in both totals.
#[derive(Debug, Default)]
struct ChangeTally {
    changes: FileChanges,
    files: BTreeSet<String>,
    by_tool_files: BTreeMap<String, BTreeSet<String>>,
}

impl ChangeTally {
    /// Read one tool result. A tool that could have edited and gave nothing
    /// readable is counted as opaque; one that is not an editor at all is not
    /// counted here in either direction.
    ///
    /// `failed` is the call's own `is_error`. A refused edit changed nothing
    /// and is a known zero — counting it as opaque would manufacture an
    /// unknown out of the one case kagviz is certain about.
    fn absorb(&mut self, tool: &str, result: &Value, failed: bool) {
        match recover_changes(tool, result) {
            Some(rec) => {
                let slot = self.changes.by_tool.entry(tool.to_string()).or_default();
                slot.calls += 1;
                slot.lines_added += rec.added;
                slot.lines_deleted += rec.deleted;
                self.changes.lines_added += rec.added;
                self.changes.lines_deleted += rec.deleted;
                let seen = self.by_tool_files.entry(tool.to_string()).or_default();
                for f in rec.files {
                    seen.insert(f.clone());
                    self.files.insert(f);
                }
                // Files known, lines not: the call still owes `opaque_edits` an
                // entry, or `lines_added` would read as a total when it is a
                // floor. The two halves are tracked separately for exactly this.
                if !rec.lines_known {
                    slot.opaque += 1;
                    self.changes.opaque_edits += 1;
                }
            }
            None if edits_files(tool) && !failed => self.opaque(tool, 1),
            None => {}
        }
    }

    /// Record `n` calls of a tool that changes files with nothing to read.
    fn opaque(&mut self, tool: &str, n: usize) {
        let slot = self.changes.by_tool.entry(tool.to_string()).or_default();
        slot.calls += n;
        slot.opaque += n;
        self.changes.opaque_edits += n;
    }

    /// Absorb another pass's tally.
    ///
    /// Totals the delegated tier by merging the *sets*, not the counts: two
    /// spawns that edited the same file changed one file between them, and
    /// adding their `files_touched` would report two.
    fn merge(&mut self, other: &ChangeTally) {
        self.changes.lines_added += other.changes.lines_added;
        self.changes.lines_deleted += other.changes.lines_deleted;
        self.changes.opaque_edits += other.changes.opaque_edits;
        for (tool, c) in &other.changes.by_tool {
            let slot = self.changes.by_tool.entry(tool.clone()).or_default();
            slot.calls += c.calls;
            slot.lines_added += c.lines_added;
            slot.lines_deleted += c.lines_deleted;
            slot.opaque += c.opaque;
        }
        self.files.extend(other.files.iter().cloned());
        for (tool, files) in &other.by_tool_files {
            self.by_tool_files
                .entry(tool.clone())
                .or_default()
                .extend(files.iter().cloned());
        }
    }

    /// The finished picture. Borrows rather than consumes, so one pass's tally
    /// can be both reported and merged into a larger one.
    fn finish(&self) -> FileChanges {
        let mut out = self.changes.clone();
        out.files_touched = self.files.len();
        for (tool, files) in &self.by_tool_files {
            if let Some(slot) = out.by_tool.get_mut(tool) {
                slot.files_touched = files.len();
            }
        }
        out.by_tool.retain(|_, c| c.calls > 0);
        out
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
        let s = summarize(None, &t, &[]);
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
        let s = summarize(None, &t, &[]);
        assert_eq!(s.phases.len(), 4);
        assert_eq!(s.activity.spans.len(), 1);
        assert_eq!(
            s.phases.iter().map(|p| p.secs).sum::<i64>(),
            s.activity.spans[0].secs,
            "phases must tile their span exactly, milliseconds and all"
        );
    }

    /// A skill invocation writes two user records and kagviz kept the wrong
    /// one: the 158-byte `<command-*>` scaffold holding what the user typed
    /// was discarded as injected, and the multi-kilobyte skill body that
    /// follows it — `isMeta: true`, matching no prefix — was counted as the
    /// user turn, becoming the prompt, the preview and a phase boundary.
    /// Corpus-wide that was 39% of counted prompts and 504 real inputs lost.
    #[test]
    fn a_skill_invocation_counts_the_users_line_not_the_harness_body() {
        let t = transcript(&[
            r#"{"type":"user","timestamp":"2026-08-20T10:00:00.000Z","message":{
                "content":"<command-message>start-sprint</command-message>\n<command-name>/start-sprint</command-name>\n<command-args>korg:1606 proceed with implementation</command-args>"}}"#,
            r#"{"type":"user","isMeta":true,"timestamp":"2026-08-20T10:00:01.000Z","message":{
                "content":[{"type":"text","text":"Base directory for this skill: /home/ken/.claude/skills/start-sprint\n\n# Start Sprint Skill\n\n## Overview\n\nlots of instructions"}]}}"#,
            r#"{"type":"assistant","timestamp":"2026-08-20T10:00:30.000Z","message":{
                "content":[{"type":"tool_use","id":"t1","name":"Read"}]}}"#,
        ]);
        let s = summarize(None, &t, &[]);

        assert_eq!(
            s.user_prompts, 1,
            "the scaffold is the prompt, the body is not"
        );
        assert_eq!(
            s.phases.len(),
            1,
            "the harness body must not cut a phase of its own"
        );
        assert_eq!(
            s.phases[0].opened_by.as_deref(),
            Some("/start-sprint korg:1606 proceed with implementation"),
            "the scaffold reconstructs what the user typed, exactly"
        );
        match &s.user_involvement[0] {
            Involvement::Prompt {
                preview, truncated, ..
            } => {
                assert_eq!(
                    preview,
                    "/start-sprint korg:1606 proceed with implementation"
                );
                assert!(!truncated);
            }
            other => panic!("expected a prompt, got {other:?}"),
        }
    }

    /// The scaffold's tags are written in whatever order the command emitted
    /// them, and every line after the first carries the caller's indentation.
    /// Both shapes are in the corpus, so the parse reads tags, not layout.
    /// A command with no arguments is just its name.
    #[test]
    fn a_command_scaffold_is_read_by_its_tags_not_its_layout() {
        let t = transcript(&[
            r#"{"type":"user","timestamp":"2026-08-20T10:00:00.000Z","message":{
                "content":"<command-name>/model</command-name>\n            <command-message>model</command-message>\n            <command-args>opus[1m]</command-args>"}}"#,
            r#"{"type":"user","timestamp":"2026-08-20T10:00:10.000Z","message":{
                "content":"<command-message>exit</command-message>\n<command-name>/exit</command-name>"}}"#,
            r#"{"type":"user","timestamp":"2026-08-20T10:00:20.000Z","message":{
                "content":"<command-message>clear</command-message>\n<command-name>/clear</command-name>\n<command-args></command-args>"}}"#,
        ]);
        let s = summarize(None, &t, &[]);
        assert_eq!(s.user_prompts, 3);
        let previews: Vec<&str> = s
            .user_involvement
            .iter()
            .map(|i| match i {
                Involvement::Prompt { preview, .. } => preview.as_str(),
                other => panic!("expected a prompt, got {other:?}"),
            })
            .collect();
        assert_eq!(previews, ["/model opus[1m]", "/exit", "/clear"]);
    }

    /// `isMeta` is the harness saying "I wrote this, not the user", and it is
    /// a strict superset of what the prefix list was reaching for. Excluding
    /// it must not cost the attachment count, which is a property of the
    /// record's blocks rather than of who authored it.
    #[test]
    fn is_meta_excludes_a_record_no_prefix_would_have_caught() {
        let t = transcript(&[
            r#"{"type":"user","isMeta":true,"timestamp":"2026-08-20T10:00:00.000Z","message":{
                "content":"Continue from where you left off."}}"#,
            r#"{"type":"user","isMeta":false,"timestamp":"2026-08-20T10:00:10.000Z","message":{
                "content":"actually, do it this way"}}"#,
            r#"{"type":"user","isMeta":true,"timestamp":"2026-08-20T10:00:20.000Z","message":{
                "content":[{"type":"image"},{"type":"text","text":"[Image: original 2160x2880]"}]}}"#,
        ]);
        let s = summarize(None, &t, &[]);
        assert_eq!(s.user_prompts, 1, "only the record the user wrote");
        // Two phases, not three: the span opens with one (nobody asked for it,
        // so no opener) and the user's line cuts the second. The trailing
        // isMeta record cuts nothing, which is the whole fix.
        assert_eq!(s.phases.len(), 2);
        assert_eq!(s.phases[0].opened_by, None);
        assert_eq!(
            s.phases[1].opened_by.as_deref(),
            Some("actually, do it this way")
        );
        assert_eq!(
            s.pasted_attachments, 1,
            "attachments are counted from the blocks, not from authorship"
        );
    }

    /// `active_secs` was `wall_secs - idle_secs` — two truncations — while the
    /// spans truncate once each, so the headline and the strip disagreed by up
    /// to 198s on the corpus's 209-span session. It is now defined as the sum
    /// of the span lengths, which makes the identity exact rather than close.
    #[test]
    fn active_secs_is_exactly_the_sum_of_the_span_lengths() {
        let t = transcript(&[
            r#"{"type":"user","timestamp":"2026-08-20T10:00:00.900Z"}"#,
            r#"{"type":"assistant","timestamp":"2026-08-20T10:00:30.800Z"}"#,
            // a two-hour break, then more work
            r#"{"type":"user","timestamp":"2026-08-20T12:00:30.700Z"}"#,
            r#"{"type":"assistant","timestamp":"2026-08-20T12:00:40.600Z"}"#,
            r#"{"type":"user","timestamp":"2026-08-20T14:00:40.500Z"}"#,
            r#"{"type":"assistant","timestamp":"2026-08-20T14:00:50.400Z"}"#,
        ]);
        let s = summarize(None, &t, &[]);
        assert_eq!(s.activity.spans.len(), 3);
        assert_eq!(
            s.active_secs,
            s.activity.spans.iter().map(|sp| sp.secs).sum::<i64>()
        );
        assert_eq!(
            s.active_secs,
            s.phases.iter().map(|p| p.secs).sum::<i64>(),
            "phases tile their spans, so they now tile active time too"
        );
        // Three stretches of 29.9s, 9.9s and 9.9s. `wall_secs - idle_secs`
        // said 51: two truncations over the whole session against one each.
        assert_eq!(s.active_secs, 47);
        assert_eq!(s.wall_secs - s.idle_secs, 51);
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
        let s = summarize(None, &transcript(&lines), &[]);
        assert_eq!(s.phases[0].kind, PhaseKind::Exploring);
        assert_eq!(s.phases[0].mix.read, 9);

        let edit = r#"{"type":"assistant","timestamp":"2026-08-20T10:00:20.000Z","message":{
                "content":[{"type":"tool_use","name":"Edit"}]}}"#;

        lines.push(edit);
        let s = summarize(None, &transcript(&lines), &[]);
        assert_eq!(s.phases[0].mix.edit, 1);
        assert_eq!(
            s.phases[0].kind,
            PhaseKind::Exploring,
            "1 edit in 10 is 10%, under the 15% bar"
        );

        lines.push(edit);
        let s = summarize(None, &transcript(&lines), &[]);
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
        let s = summarize(None, &t, &[]);
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
        let s = summarize(None, &t, &[]);
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
        let s = summarize(None, &t, &[]);
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
        let s = summarize(None, &t, &[]);
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
        let s = summarize(None, &t, &[]);
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
        assert_eq!(summarize(None, &t, &[]).user_prompts, 2);
    }

    /// `promptId` rides on harness-injected records too, so it cannot be the
    /// discriminator. Everything below is the harness talking, not the user.
    ///
    /// A `<command-*>` scaffold is deliberately *not* in this list any more —
    /// it is the one thing on the old prefix list that was the user speaking.
    /// See `a_command_scaffold_is_read_by_its_tags_not_its_layout`.
    #[test]
    fn harness_injected_context_is_not_user_involvement() {
        let t = transcript(&[
            r#"{"type":"user","promptId":"p1","message":{"content":
                "<local-command-caveat>Caveat: the messages below"}}"#,
            r#"{"type":"user","promptId":"p1","message":{"content":
                "[Image: original 2160x2880, displayed at 1500x2000.]"}}"#,
            r#"{"type":"user","promptId":"p1","message":{"content":[
                {"type":"text","text":"<ide_opened_file>The user opened a file"}]}}"#,
            r#"{"type":"user","promptId":"p1","message":{"content":[
                {"type":"text","text":"<system-reminder>be good</system-reminder>"}]}}"#,
        ]);
        assert_eq!(summarize(None, &t, &[]).user_prompts, 0);
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
        let s = summarize(None, &t, &[]);
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
        let s = summarize(None, &t, &[]);
        assert_eq!(s.changes.lines_added, 5);
        assert_eq!(s.changes.lines_deleted, 1);
        assert_eq!(s.changes.files_touched, 2);
    }

    /// The adapter table's second entry. A file-serving MCP tool returns its
    /// own unified diff, wrapped in a JSON *string* rather than an object —
    /// measured shape, `mcp__kaed-kai__edit`. Before the table these calls were
    /// invisible in both directions: not in the deltas, and not in
    /// `opaque_edits` either, because only `Bash` was ever named as opaque.
    #[test]
    fn a_returned_unified_diff_is_recovered_rather_than_left_opaque() {
        let envelope = r#"{\"applied\":true,\"diff\":\"--- a/m.yml\\n+++ b/m.yml\\n@@ -1,3 +1,4 @@\\n ctx\\n-gone\\n+one\\n+two\\n\",\"files\":[{\"path\":\"m.yml\"}]}"#;
        let t = transcript(&[
            r#"{"type":"assistant","timestamp":"2026-08-20T10:00:00.000Z","message":{
                "content":[{"type":"tool_use","id":"t1","name":"mcp__kaed-kai__edit"}]}}"#,
            &format!(
                r#"{{"type":"user","timestamp":"2026-08-20T10:00:05.000Z","toolUseResult":"{envelope}",
                "message":{{"content":[{{"type":"tool_result","tool_use_id":"t1"}}]}}}}"#
            ),
        ]);
        let s = summarize(None, &t, &[]);
        assert_eq!(s.changes.lines_added, 2);
        assert_eq!(s.changes.lines_deleted, 1);
        assert_eq!(s.changes.files_touched, 1);
        assert_eq!(s.changes.opaque_edits, 0);

        let by = &s.changes.by_tool["mcp__kaed-kai__edit"];
        assert_eq!((by.calls, by.opaque, by.lines_added), (1, 0, 2));
    }

    /// An edit the server refused changed nothing, and kagviz knows that
    /// exactly. Calling it opaque would manufacture an unknown out of the one
    /// case there is no doubt about.
    #[test]
    fn a_refused_edit_is_a_known_zero_not_an_unknown() {
        let t = transcript(&[
            r#"{"type":"assistant","message":{
                "content":[{"type":"tool_use","id":"t1","name":"mcp__kaed-kai__edit"}]}}"#,
            r#"{"type":"user","toolUseResult":"{\"applied\":false}",
                "message":{"content":[{"type":"tool_result","tool_use_id":"t1"}]}}"#,
        ]);
        let s = summarize(None, &t, &[]);
        assert_eq!(s.changes.opaque_edits, 0);
        assert_eq!(s.changes.lines_added, 0);
        assert_eq!(s.changes.by_tool["mcp__kaed-kai__edit"].calls, 1);
    }

    /// Found in the cleo corpus, not by reasoning: 16 kaed results are
    /// `{"applied":true,"files":[…]}` with **no `diff`**. The edit landed and
    /// named its files exactly; only the line counts are missing.
    ///
    /// Both halves have to be reported honestly at once — take the files
    /// (they are exact, and dropping them under-reported `files_touched` by 36
    /// paths corpus-wide), and still charge `opaque_edits`, because otherwise
    /// `lines_added` reads as a total when it is a floor.
    #[test]
    fn a_result_naming_files_without_a_diff_yields_files_and_an_unknown() {
        let envelope = r#"{\"applied\":true,\"files\":[{\"path\":\"a.rs\"},{\"path\":\"b.rs\"}],\"txn_id\":101}"#;
        let t = transcript(&[
            r#"{"type":"assistant","message":{
                "content":[{"type":"tool_use","id":"t1","name":"mcp__kaed-kai__edit"}]}}"#,
            &format!(
                r#"{{"type":"user","toolUseResult":"{envelope}",
                "message":{{"content":[{{"type":"tool_result","tool_use_id":"t1"}}]}}}}"#
            ),
        ]);
        let s = summarize(None, &t, &[]);
        assert_eq!(
            s.changes.files_touched, 2,
            "the files are exact — keep them"
        );
        assert_eq!(s.changes.lines_added, 0);
        assert_eq!(
            s.changes.opaque_edits, 1,
            "lines are unknown, so the line counts must not read as a total"
        );

        // The audit surface has to show both at once, or a reader cannot tell
        // that `files_touched` is exact while `lines_added` is a floor.
        let by = &s.changes.by_tool["mcp__kaed-kai__edit"];
        assert_eq!((by.calls, by.opaque, by.files_touched), (1, 1, 2));
    }

    /// A tool that edits files and hands back something kagviz cannot read is
    /// an unknown. The corpus has no such call today — every unreadable `Edit`
    /// result in it is a failed one — so this guards the drift, not the present.
    #[test]
    fn an_unreadable_edit_result_is_opaque_but_a_failed_one_is_not() {
        let t = transcript(&[
            r#"{"type":"assistant","message":{"content":[
                {"type":"tool_use","id":"t1","name":"Edit"},
                {"type":"tool_use","id":"t2","name":"Edit"}]}}"#,
            // Unreadable and did not error: kagviz cannot say it changed nothing.
            r#"{"type":"user","toolUseResult":"some future shape",
                "message":{"content":[{"type":"tool_result","tool_use_id":"t1"}]}}"#,
            // Refused: it changed nothing, and that is already in tool_failures.
            r#"{"type":"user","toolUseResult":"Error: String to replace not found in file.",
                "message":{"content":[{"type":"tool_result","tool_use_id":"t2","is_error":true}]}}"#,
        ]);
        let s = summarize(None, &t, &[]);
        assert_eq!(s.changes.opaque_edits, 1);
        assert_eq!(
            s.changes.by_tool["Edit"],
            ToolChanges {
                calls: 1,
                opaque: 1,
                ..ToolChanges::default()
            }
        );
        assert_eq!(s.tool_failures["Edit"], 1);
    }

    /// Markdown is full of `---`, and a deleted line whose content is `--`
    /// arrives in a diff as `---`. Counting `+`/`-` by prefix eats it. Only
    /// lines inside a hunk are counted, and the file-header pair is matched as
    /// a pair of paths.
    #[test]
    fn a_deleted_dashes_line_is_not_mistaken_for_a_diff_header() {
        let diff = "--- a/doc.md\n+++ b/doc.md\n@@ -1,4 +1,3 @@\n---\n-title: x\n----\n+++\n ctx\n";
        // Three removals (`---`, `-title: x`, `----`) and one addition (`+++`).
        assert_eq!(count_unified(diff), (1, 3));
    }

    /// A diff over several files: each header pair closes the hunk before it,
    /// so the second file's `---`/`+++` are not counted as changed lines.
    #[test]
    fn a_multi_file_unified_diff_counts_each_files_hunks() {
        let diff = "--- a/one\n+++ b/one\n@@ -1 +1,2 @@\n ctx\n+added\n\
                    --- a/two\n+++ b/two\n@@ -1,2 +1 @@\n-gone\n ctx\n";
        assert_eq!(count_unified(diff), (1, 1));
    }

    /// `by_tool` is the audit surface: the totals must be the parts, or it is
    /// decoration rather than a check.
    #[test]
    fn the_per_tool_breakdown_adds_up_to_the_totals() {
        let t = transcript(&[
            r#"{"type":"assistant","message":{"content":[
                {"type":"tool_use","id":"t1","name":"Edit"},
                {"type":"tool_use","id":"t2","name":"Bash"},
                {"type":"tool_use","id":"t3","name":"Bash"}]}}"#,
            r#"{"type":"user","toolUseResult":{"filePath":"/a.rs","structuredPatch":[
                {"lines":["+one","+two","-three"]}]},
                "message":{"content":[{"type":"tool_result","tool_use_id":"t1"}]}}"#,
        ]);
        let s = summarize(None, &t, &[]);
        let c = &s.changes;
        assert_eq!(
            c.lines_added,
            c.by_tool.values().map(|t| t.lines_added).sum::<usize>()
        );
        assert_eq!(
            c.lines_deleted,
            c.by_tool.values().map(|t| t.lines_deleted).sum::<usize>()
        );
        assert_eq!(
            c.opaque_edits,
            c.by_tool.values().map(|t| t.opaque).sum::<usize>()
        );
        assert_eq!(c.by_tool["Bash"].calls, 2);
        assert_eq!(c.by_tool["Bash"].opaque, 2);
        assert_eq!(c.by_tool["Edit"].opaque, 0);
    }

    #[test]
    fn shell_calls_are_reported_as_opaque_rather_than_as_no_change() {
        let t = transcript(&[r#"{"type":"assistant","message":{"content":[
                {"type":"tool_use","id":"t1","name":"Bash"}]}}"#]);
        let s = summarize(None, &t, &[]);
        assert_eq!(s.changes.files_touched, 0);
        assert_eq!(s.changes.opaque_edits, 1);
    }

    fn subagent(agent_id: &str, lines: &[&str]) -> Subagent {
        Subagent {
            agent_id: Some(agent_id.to_string()),
            transcript: transcript(lines),
        }
    }

    /// The rollup, and the rule it is built on: delegated work is a **tier**,
    /// not an addend. The parent's own numbers must come out of this exactly
    /// as they went in — a session that spawned an agent still made one `Agent`
    /// call — and the delegated cost stands beside them, with the sum spelled
    /// out rather than left to the reader.
    #[test]
    fn a_subagents_work_is_a_separate_tier_and_never_moves_the_parents_numbers() {
        let parent = transcript(&[
            r#"{"type":"user","timestamp":"2026-08-20T10:00:00.000Z","message":{
                "content":"map the linking layer"}}"#,
            r#"{"type":"assistant","timestamp":"2026-08-20T10:00:05.000Z","message":{
                "usage":{"output_tokens":100},
                "content":[{"type":"tool_use","id":"t1","name":"Agent",
                            "input":{"subagent_type":"Explore"}}]}}"#,
            r#"{"type":"user","timestamp":"2026-08-20T10:00:40.000Z","toolUseResult":{
                "agentId":"a1","description":"Map the linking layer",
                "resolvedModel":"claude-opus-5"},
                "message":{"content":[{"type":"tool_result","tool_use_id":"t1"}]}}"#,
        ]);
        let spawned = subagent(
            "a1",
            &[
                r#"{"type":"assistant","isSidechain":true,"agentId":"a1",
                    "timestamp":"2026-08-20T10:00:10.000Z","message":{
                    "usage":{"output_tokens":900},
                    "content":[{"type":"tool_use","id":"s1","name":"Grep"},
                               {"type":"tool_use","id":"s2","name":"Read"}]}}"#,
                r#"{"type":"user","isSidechain":true,"agentId":"a1",
                    "timestamp":"2026-08-20T10:00:30.000Z","message":{
                    "content":[{"type":"tool_result","tool_use_id":"s1","is_error":true}]}}"#,
            ],
        );

        let alone = summarize(None, &parent, &[]);
        let s = summarize(None, &parent, std::slice::from_ref(&spawned));

        // Not one number of the parent's moved because an agent was folded in.
        assert_eq!(s.tool_calls, alone.tool_calls);
        assert_eq!(s.tokens.output, alone.tokens.output);
        assert_eq!(s.assistant_turns, alone.assistant_turns);
        assert_eq!(s.phases.len(), alone.phases.len());
        assert_eq!(s.total_tool_calls(), 1, "the parent made one Agent call");

        let d = &s.delegation;
        assert_eq!(d.spawns.len(), 1);
        assert_eq!(d.unjoined_spawns, 0);
        assert_eq!(d.inline_records, 0);

        // Joined to the parent's Agent call, so the tier can say what it was for.
        let spawn = &d.spawns[0];
        assert_eq!(spawn.agent_id.as_deref(), Some("a1"));
        assert_eq!(spawn.subagent_type.as_deref(), Some("Explore"));
        assert_eq!(spawn.description.as_deref(), Some("Map the linking layer"));
        assert_eq!(spawn.model.as_deref(), Some("claude-opus-5"));
        assert!(spawn.sidecar);
        assert_eq!(spawn.tool_calls["Grep"], 1);
        assert_eq!(spawn.tool_failures["Grep"], 1);
        assert_eq!(spawn.active_secs, 20);

        // Two tiers, and the sum said out loud.
        assert_eq!(d.totals.tokens.output, 900);
        assert_eq!(s.combined_tool_calls(), 3);
        assert_eq!(s.combined_tool_failures(), 1);
        assert_eq!(s.combined_output_tokens(), 1000);
    }

    /// The format drift. Older CLI versions inlined subagent turns into the
    /// main transcript with `isSidechain` instead of writing a sidecar. Those
    /// records are *not* the parent's work, and leaving them in its counts is
    /// the same undercount wearing the opposite sign — the parent looks like it
    /// did the delegated work itself.
    ///
    /// No transcript in the kai corpus takes this branch, so this test is the
    /// only thing holding it.
    #[test]
    fn inlined_sidechain_turns_are_lifted_out_of_the_parent_into_the_tier() {
        let t = transcript(&[
            r#"{"type":"assistant","timestamp":"2026-08-20T10:00:00.000Z","message":{
                "usage":{"output_tokens":10},
                "content":[{"type":"tool_use","id":"t1","name":"Task",
                            "input":{"subagent_type":"Explore"}}]}}"#,
            r#"{"type":"assistant","isSidechain":true,"agentId":"old1",
                "timestamp":"2026-08-20T10:00:10.000Z","message":{
                "usage":{"output_tokens":500},
                "content":[{"type":"tool_use","id":"s1","name":"Bash"}]}}"#,
            r#"{"type":"assistant","isSidechain":true,"agentId":"old1",
                "timestamp":"2026-08-20T10:00:20.000Z","message":{
                "content":[{"type":"tool_use","id":"s2","name":"Read"}]}}"#,
        ]);
        let s = summarize(None, &t, &[]);

        // The parent kept its own Task call and nothing else.
        assert_eq!(s.total_tool_calls(), 1);
        assert_eq!(s.tool_calls["Task"], 1);
        assert!(!s.tool_calls.contains_key("Bash"));
        assert_eq!(s.tokens.output, 10);
        assert_eq!(s.assistant_turns, 1);
        // And the sidechain records never reached a phase or the strip.
        assert_eq!(s.phases.len(), 1);
        assert_eq!(s.phases[0].mix.run, 0);

        let d = &s.delegation;
        assert_eq!(d.inline_records, 2, "the move is reported, not silent");
        assert_eq!(d.spawns.len(), 1);
        assert!(!d.spawns[0].sidecar);
        assert_eq!(d.spawns[0].agent_id.as_deref(), Some("old1"));
        assert_eq!(d.totals.tokens.output, 500);
        // A subagent's shell call is as opaque as the parent's.
        assert_eq!(d.totals.changes.opaque_edits, 1);
        assert_eq!(s.combined_tool_calls(), 3);
    }

    /// A spawn whose transcript is not on disk. The work happened; kagviz
    /// cannot see it. Reporting zero delegated calls would read as "it
    /// delegated nothing", which is the failure this whole document is
    /// organised against.
    #[test]
    fn a_spawn_with_no_transcript_is_counted_as_unknown_not_as_nothing() {
        let t = transcript(&[r#"{"type":"assistant","message":{"content":[
                {"type":"tool_use","id":"t1","name":"Agent","input":{"subagent_type":"Explore"}},
                {"type":"tool_use","id":"t2","name":"Agent","input":{"subagent_type":"Plan"}}]}}"#]);
        let s = summarize(None, &t, &[]);
        assert_eq!(s.delegation.unjoined_spawns, 2);
        assert!(s.delegation.spawns.is_empty());
        assert!(!s.delegation.is_empty(), "unknown work is still a tier");
    }

    /// Two spawns that edited the same file changed one file between them.
    /// The tier totals merge the path sets rather than adding the counts.
    #[test]
    fn the_tier_totals_merge_files_rather_than_adding_them() {
        let edit = |id: &str| {
            format!(
                r#"{{"type":"user","message":{{"content":[
                    {{"type":"tool_result","tool_use_id":"{id}"}}]}},
                    "toolUseResult":{{"filePath":"/shared.rs","structuredPatch":[
                    {{"lines":["+one"]}}]}}}}"#
            )
        };
        let call = |id: &str| {
            format!(
                r#"{{"type":"assistant","message":{{"content":[
                    {{"type":"tool_use","id":"{id}","name":"Edit"}}]}}}}"#
            )
        };
        let a = subagent("a1", &[&call("x1"), &edit("x1")]);
        let b = subagent("a2", &[&call("y1"), &edit("y1")]);
        let s = summarize(None, &transcript(&[]), &[a, b]);

        let c = &s.delegation.totals.changes;
        assert_eq!(c.files_touched, 1, "one file, edited by two agents");
        assert_eq!(c.lines_added, 2);
        assert_eq!(c.by_tool["Edit"].calls, 2);
        assert_eq!(c.by_tool["Edit"].files_touched, 1);
    }

    #[test]
    fn the_activity_series_splits_at_idle_gaps() {
        let t = transcript(&[
            r#"{"type":"user","timestamp":"2026-08-20T10:00:00.000Z"}"#,
            r#"{"type":"assistant","timestamp":"2026-08-20T10:00:30.000Z"}"#,
            r#"{"type":"user","timestamp":"2026-08-20T12:00:30.000Z"}"#,
            r#"{"type":"assistant","timestamp":"2026-08-20T12:00:40.000Z"}"#,
        ]);
        let a = summarize(None, &t, &[]).activity;
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
        assert_eq!(summarize(None, &short, &[]).activity.bucket_secs, 5);

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
        let a = summarize(None, &transcript(&refs), &[]).activity;
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
        let s = summarize(None, &t, &[]);
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
        let s = summarize(None, &t, &[]);
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
        let s = summarize(None, &t, &[]);
        assert_eq!(s.ask_user_questions, 1);
        assert_eq!(s.skills, vec!["sprint-ship"]);
        assert_eq!(s.subagents, vec!["Explore"]);
    }
}
