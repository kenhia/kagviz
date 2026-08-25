//! Tolerant reader for Claude Code session transcripts (`*.jsonl`).
//!
//! The on-disk format is append-only JSON Lines, and a single session
//! directory can hold records written by several CLI versions. Records are
//! therefore read *permissively*: unknown `type` values and unknown fields are
//! kept rather than rejected, because the format drifts under us. See
//! `docs/transcript-format.md` for what the records actually contain.

use crate::discover::SessionPaths;
use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Read a field that a CLI version may write as an explicit `null` where a
/// number or object is expected.
///
/// `#[serde(default)]` covers an *absent* field only — a present `null` is
/// still handed to the field's own deserializer, which rejects it and takes
/// the whole record down with it. Observed in the wild as
/// `"output_tokens_details": null`; one such line silently drops a turn's
/// tokens, tool calls and timestamp from every number downstream.
fn null_as_default<'de, D, T>(de: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(de)?.unwrap_or_default())
}

/// One transcript line.
///
/// Only the fields kagviz relies on are typed; everything else lands in
/// `rest`, so a newer CLI adding a field can never fail the parse.
#[derive(Debug, Deserialize)]
pub struct Record {
    #[serde(rename = "type")]
    pub kind: String,
    pub timestamp: Option<String>,
    /// Record identity. Parsed now, read once the timeline lands (it is what
    /// `parentUuid` chains against).
    #[expect(dead_code)]
    pub uuid: Option<String>,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
    pub version: Option<String>,
    pub cwd: Option<String>,
    #[serde(rename = "gitBranch")]
    pub git_branch: Option<String>,
    /// Older CLI versions inlined subagent turns into the main transcript and
    /// flagged them here; newer ones write `subagents/` sidecar files instead.
    /// Both shapes reach the delegated tier — see `summary::Delegation`.
    #[serde(rename = "isSidechain")]
    pub is_sidechain: Option<bool>,
    /// The harness marking a record it wrote into the user channel itself:
    /// a skill body, a command caveat, an attachment placeholder, a resume
    /// nudge. Set on `user` records only, and written as an explicit `false`
    /// as often as it is omitted — hence `Option`, read through
    /// [`Record::is_harness_written`].
    #[serde(rename = "isMeta")]
    pub is_meta: Option<bool>,
    /// Which spawned agent a record belongs to. Present on every record of a
    /// `subagents/agent-*.jsonl` sidecar, and on inlined sidechain records.
    /// It is what joins a subagent's work back to the parent's `Agent` call.
    #[serde(rename = "agentId")]
    pub agent_id: Option<String>,
    /// Groups every record belonging to one user turn — including tool
    /// results and harness-injected context. Emphatically *not* a marker of
    /// user authorship; see [`INJECTED_PREFIXES`].
    #[serde(rename = "promptId")]
    #[allow(dead_code, reason = "turn grouping, used once the timeline lands")]
    pub prompt_id: Option<String>,
    pub message: Option<Message>,
    /// Tool-specific result payload. Shape varies per tool, so it stays raw.
    #[serde(rename = "toolUseResult")]
    pub tool_use_result: Option<Value>,
    /// Every field kagviz does not yet model, kept so a newer CLI's records
    /// survive a round trip. Read by tests today, by the timeline later.
    #[serde(flatten)]
    #[allow(dead_code, reason = "format surface held deliberately ahead of use")]
    pub rest: Map<String, Value>,
}

impl Record {
    /// Whether the harness wrote this record rather than the user.
    ///
    /// The load-bearing half of telling a prompt from an injection. A skill
    /// invocation writes the user's line and then hands the agent a
    /// multi-kilobyte instruction document; only the second is flagged, and
    /// without this the document is what gets counted. Absent means "not
    /// flagged", which is the same answer as an explicit `false`.
    pub fn is_harness_written(&self) -> bool {
        self.is_meta.unwrap_or(false)
    }
}

#[derive(Debug, Deserialize)]
pub struct Message {
    #[expect(dead_code)]
    pub role: Option<String>,
    pub model: Option<String>,
    pub usage: Option<Usage>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub content: Content,
}

/// `content` is a bare string on user prompts and a block array on everything
/// else.
#[derive(Debug, Deserialize, Default)]
#[serde(untagged)]
pub enum Content {
    /// The prompt text itself. Captured so the string and block forms stay
    /// distinguishable; read once prompts are rendered on the timeline.
    Text(#[allow(dead_code, reason = "read by tests; rendered once prompts land")] String),
    Blocks(Vec<Block>),
    #[default]
    Empty,
}

impl Content {
    /// The blocks, or an empty slice for the string / absent forms.
    pub fn blocks(&self) -> &[Block] {
        match self {
            Content::Blocks(b) => b,
            _ => &[],
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Block {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: Option<String>,
    pub name: Option<String>,
    pub input: Option<Value>,
    pub tool_use_id: Option<String>,
    pub is_error: Option<bool>,
    pub text: Option<String>,
}

impl Block {
    /// True when this block is text the *harness* injected into the user
    /// channel rather than something the user wrote.
    pub fn is_injected_context(&self) -> bool {
        self.text
            .as_deref()
            .map(str::trim_start)
            .is_some_and(|t| INJECTED_PREFIXES.iter().any(|p| t.starts_with(p)))
    }
}

/// Markers for content the harness writes into the user channel: IDE state,
/// system reminders, local-command output, attachment placeholders.
///
/// None of it is the user saying something, and all of it arrives on `user`
/// records carrying a `promptId` — which is why `promptId` cannot be used to
/// find prompts. It marks the turn a record belongs to, not its authorship.
///
/// The list is the *narrow* half of that job and shrinking:
/// [`Record::is_harness_written`] catches injections by the flag the harness
/// sets, and is a strict superset of several entries that used to be here.
/// The `<command-*>` tags are deliberately **absent** — they are not noise but
/// structure, and [`command_line`] reads the user's own line back out of them.
pub const INJECTED_PREFIXES: &[&str] = &[
    "<system-reminder>",
    "<ide_opened_file>",
    "<ide_selection>",
    "<ide_diagnostics>",
    "<local-command-caveat>",
    "<local-command-stdout>",
    "<local-command-stderr>",
    "[Image:",
    "[Request interrupted",
];

/// The command line the user typed, read back out of the scaffold a slash
/// command writes into the user channel.
///
/// A slash command emits one `user` record holding `<command-name>`,
/// `<command-message>` and — usually — `<command-args>`. That record *is* the
/// user's input, 158 bytes of it, and reconstructing it from the tags is
/// exact where prefix-stripping the instruction document that follows would be
/// a guess. `<command-message>` is the name without its slash and is ignored.
///
/// Tags are read by name rather than by position: the corpus carries both tag
/// orders, every line after the first may be indented by the emitting command,
/// and `<command-args>` is sometimes present but empty (`/clear`) and
/// sometimes absent entirely (`/exit`). Both mean "no arguments".
///
/// Returns `None` when there is no `<command-name>`, which is how a caller
/// asks "is this a command scaffold at all?".
pub fn command_line(text: &str) -> Option<String> {
    let name = tag_value(text, "command-name")?.trim();
    if name.is_empty() {
        return None;
    }
    match tag_value(text, "command-args").map(str::trim) {
        Some(args) if !args.is_empty() => Some(format!("{name} {args}")),
        _ => Some(name.to_string()),
    }
}

/// The text between `<tag>` and `</tag>`, or `None` if the pair is not there.
fn tag_value<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)? + start;
    Some(&text[start..end])
}

/// Per-turn token counts.
///
/// Every field is `null`-tolerant, not merely optional: these are the numbers
/// a drifting format is most likely to blank out, and losing a whole record to
/// one of them is the worst possible trade.
#[derive(Debug, Deserialize, Default, Clone, Copy)]
pub struct Usage {
    #[serde(default, deserialize_with = "null_as_default")]
    pub input_tokens: u64,
    #[serde(default, deserialize_with = "null_as_default")]
    pub output_tokens: u64,
    #[serde(default, deserialize_with = "null_as_default")]
    pub cache_read_input_tokens: u64,
    #[serde(default, deserialize_with = "null_as_default")]
    pub cache_creation_input_tokens: u64,
    #[serde(default, deserialize_with = "null_as_default")]
    pub output_tokens_details: OutputTokenDetails,
}

#[derive(Debug, Deserialize, Default, Clone, Copy)]
pub struct OutputTokenDetails {
    #[serde(default, deserialize_with = "null_as_default")]
    pub thinking_tokens: u64,
}

/// The result of reading one transcript file.
///
/// `skipped` is reported rather than swallowed: a line kagviz could not parse
/// is a gap in every number downstream, and a silent skip would present a
/// partial reading as a complete one.
#[derive(Debug)]
pub struct Transcript {
    pub records: Vec<Record>,
    pub skipped: usize,
}

/// A subagent transcript read from a `subagents/agent-*.jsonl` sidecar.
#[derive(Debug)]
pub struct Subagent {
    /// Taken from the file name. The records carry the same id in `agentId`,
    /// and that one wins where present — the name is the fallback for a CLI
    /// that stops writing the field.
    pub agent_id: Option<String>,
    pub transcript: Transcript,
}

/// Read one subagent sidecar, recovering its agent id from the file name.
pub fn read_subagent(path: &Path) -> Result<Subagent> {
    let agent_id = path
        .file_stem()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix("agent-"))
        .map(str::to_string);
    Ok(Subagent {
        agent_id,
        transcript: read(path)?,
    })
}

/// Read a session's transcript and every subagent sidecar beside it.
///
/// The sidecars are read here, at the edge, so `summarize` stays a pure
/// function of bytes handed to it rather than of what happens to be on disk.
pub fn read_session(session: &SessionPaths) -> Result<(Transcript, Vec<Subagent>)> {
    let t = read(&session.transcript)?;
    let subagents = session
        .subagents
        .iter()
        .map(|p| read_subagent(p))
        .collect::<Result<Vec<_>>>()?;
    Ok((t, subagents))
}

/// Read a transcript, skipping (and counting) lines that will not parse.
pub fn read(path: &Path) -> Result<Transcript> {
    let file =
        File::open(path).with_context(|| format!("opening transcript {}", path.display()))?;
    let mut records = Vec::new();
    let mut skipped = 0;

    for line in BufReader::new(file).lines() {
        let line = line.with_context(|| format!("reading {}", path.display()))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Record>(line) {
            Ok(record) => records.push(record),
            Err(_) => skipped += 1,
        }
    }

    Ok(Transcript { records, skipped })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_string_content_user_prompt() {
        let line = r#"{"type":"user","promptId":"p1","message":{"role":"user","content":"hello"}}"#;
        let rec: Record = serde_json::from_str(line).unwrap();
        assert_eq!(rec.kind, "user");
        assert!(rec.prompt_id.is_some());
        let content = &rec.message.unwrap().content;
        assert!(matches!(content, Content::Text(t) if t == "hello"));
        assert!(content.blocks().is_empty());
    }

    #[test]
    fn parses_tool_use_blocks_and_usage() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","model":"claude-opus-5",
            "usage":{"output_tokens":10,"output_tokens_details":{"thinking_tokens":4}},
            "content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}}]}}"#;
        let rec: Record = serde_json::from_str(line).unwrap();
        let msg = rec.message.unwrap();
        assert_eq!(msg.model.as_deref(), Some("claude-opus-5"));
        let usage = msg.usage.unwrap();
        assert_eq!(usage.output_tokens, 10);
        assert_eq!(usage.output_tokens_details.thinking_tokens, 4);
        assert_eq!(msg.content.blocks()[0].name.as_deref(), Some("Bash"));
    }

    /// Found in the corpus: one CLI version writes
    /// `"output_tokens_details": null`. Serde's `default` does not cover a
    /// present `null`, so the record used to be dropped whole — taking its
    /// tool calls and timestamp with it, not just its token counts.
    #[test]
    fn a_null_where_a_number_belongs_does_not_drop_the_record() {
        let line = r#"{"type":"assistant","timestamp":"2026-08-20T10:00:00.000Z","message":{
            "usage":{"output_tokens":7,"output_tokens_details":null,"input_tokens":null},
            "content":[{"type":"tool_use","id":"t1","name":"Bash"}]}}"#;
        let rec: Record = serde_json::from_str(line).unwrap();
        let usage = rec.message.as_ref().unwrap().usage.unwrap();
        assert_eq!(usage.output_tokens, 7);
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens_details.thinking_tokens, 0);
        assert_eq!(rec.message.unwrap().content.blocks().len(), 1);
    }

    #[test]
    fn a_null_content_reads_as_no_content_rather_than_a_parse_failure() {
        let line = r#"{"type":"user","message":{"role":"user","content":null}}"#;
        let rec: Record = serde_json::from_str(line).unwrap();
        assert!(matches!(rec.message.unwrap().content, Content::Empty));
    }

    #[test]
    fn unknown_record_types_and_fields_survive() {
        let line = r#"{"type":"some-future-record","brandNewField":{"a":1}}"#;
        let rec: Record = serde_json::from_str(line).unwrap();
        assert_eq!(rec.kind, "some-future-record");
        assert!(rec.rest.contains_key("brandNewField"));
    }
}
