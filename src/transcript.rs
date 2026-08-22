//! Tolerant reader for Claude Code session transcripts (`*.jsonl`).
//!
//! The on-disk format is append-only JSON Lines, and a single session
//! directory can hold records written by several CLI versions. Records are
//! therefore read *permissively*: unknown `type` values and unknown fields are
//! kept rather than rejected, because the format drifts under us. See
//! `docs/transcript-format.md` for what the records actually contain.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

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
    #[serde(rename = "isSidechain")]
    #[expect(dead_code)]
    pub is_sidechain: Option<bool>,
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

#[derive(Debug, Deserialize)]
pub struct Message {
    #[expect(dead_code)]
    pub role: Option<String>,
    pub model: Option<String>,
    pub usage: Option<Usage>,
    #[serde(default)]
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
/// slash-command scaffolding, system reminders, attachment placeholders.
///
/// None of it is the user saying something, and all of it arrives on `user`
/// records carrying a `promptId` — which is why `promptId` cannot be used to
/// find prompts. It marks the turn a record belongs to, not its authorship.
pub const INJECTED_PREFIXES: &[&str] = &[
    "<system-reminder>",
    "<ide_opened_file>",
    "<ide_selection>",
    "<ide_diagnostics>",
    "<local-command-caveat>",
    "<local-command-stdout>",
    "<local-command-stderr>",
    "<command-name>",
    "<command-message>",
    "<command-args>",
    "[Image:",
    "[Request interrupted",
];

#[derive(Debug, Deserialize, Default, Clone, Copy)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub output_tokens_details: OutputTokenDetails,
}

#[derive(Debug, Deserialize, Default, Clone, Copy)]
pub struct OutputTokenDetails {
    #[serde(default)]
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

    #[test]
    fn unknown_record_types_and_fields_survive() {
        let line = r#"{"type":"some-future-record","brandNewField":{"a":1}}"#;
        let rec: Record = serde_json::from_str(line).unwrap();
        assert_eq!(rec.kind, "some-future-record");
        assert!(rec.rest.contains_key("brandNewField"));
    }
}
