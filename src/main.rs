//! kagviz — deterministic insight into how an agent session went.
//!
//! Reads Claude Code session transcripts and reports what actually happened.
//! Every number is derived from the transcript bytes, never inferred.

mod discover;
mod summary;
mod transcript;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "kagviz",
    version,
    about = "Visualize how an agent session went"
)]
struct Cli {
    /// Transcript root (defaults to <home>/.claude/projects).
    #[arg(long, global = true)]
    root: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List discoverable sessions.
    Sessions {
        /// Only sessions under this project slug.
        #[arg(long)]
        project: Option<String>,
    },
    /// Summarize one session by id.
    Show {
        session_id: String,
        /// Emit the facts document as JSON.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = match cli.root {
        Some(r) => r,
        None => discover::default_root()?,
    };

    match cli.command {
        Command::Sessions { project } => list_sessions(&root, project.as_deref()),
        Command::Show { session_id, json } => show_session(&root, &session_id, json),
    }
}

fn list_sessions(root: &Path, project: Option<&str>) -> Result<()> {
    let sessions = discover::sessions(root, project)?;
    println!(
        "{:<38} {:<22} {:>7} {:>7} {:>6}",
        "SESSION", "PROJECT", "ACTIVE", "TOOLS", "FAIL"
    );
    for session in &sessions {
        let t = transcript::read(&session.transcript)?;
        let s = summary::summarize(Some(session), &t);
        println!(
            "{:<38} {:<22} {:>7} {:>7} {:>6}",
            session.id,
            truncate(&session.project, 22),
            format_mins(s.active_secs),
            s.total_tool_calls(),
            s.total_tool_failures(),
        );
    }
    eprintln!("\n{} session(s) under {}", sessions.len(), root.display());
    Ok(())
}

fn show_session(root: &Path, id: &str, json: bool) -> Result<()> {
    let session = discover::sessions(root, None)?
        .into_iter()
        .find(|s| s.id == id)
        .with_context(|| format!("no session {id} under {}", root.display()))?;
    let t = transcript::read(&session.transcript)?;
    let s = summary::summarize(Some(&session), &t);

    if json {
        println!("{}", serde_json::to_string_pretty(&s)?);
        return Ok(());
    }

    println!("session   {}", session.id);
    println!("project   {}", session.project);
    if let Some(cwd) = &s.cwd {
        println!("cwd       {cwd}");
    }
    println!("cli       {}", join(&sorted(&s.cli_versions)));
    println!(
        "models    {}",
        join(&s.models.keys().cloned().collect::<Vec<_>>())
    );
    println!(
        "time      {} active / {} wall ({} idle)",
        format_mins(s.active_secs),
        format_mins(s.wall_secs),
        format_mins(s.idle_secs)
    );
    println!(
        "turns     {} assistant, {} user prompts",
        s.assistant_turns, s.user_prompts
    );
    println!(
        "tools     {} calls, {} failed",
        s.total_tool_calls(),
        s.total_tool_failures()
    );
    for (name, n) in &s.tool_calls {
        let failed = s.tool_failures.get(name).copied().unwrap_or(0);
        let note = if failed > 0 {
            format!("  ({failed} failed)")
        } else {
            String::new()
        };
        println!("            {n:>4}  {name}{note}");
    }
    println!(
        "files     {} touched, +{}/-{}  [{} opaque shell call(s) unaccounted]",
        s.changes.files_touched,
        s.changes.lines_added,
        s.changes.lines_deleted,
        s.changes.opaque_edits
    );
    println!(
        "tokens    {} out ({} thinking), {} cache read",
        s.tokens.output, s.tokens.thinking, s.tokens.cache_read
    );
    if s.pasted_attachments > 0 {
        println!("pasted    {} image(s)/document(s)", s.pasted_attachments);
    }
    if s.ask_user_questions > 0 {
        println!("asked     {} question(s) of the user", s.ask_user_questions);
    }
    if !s.skills.is_empty() {
        println!("skills    {}", join(&s.skills));
    }
    if !s.subagents.is_empty() {
        println!(
            "subagents {}  ({} transcript(s))",
            join(&s.subagents),
            s.subagent_transcripts
        );
    }
    if s.skipped_lines > 0 {
        eprintln!(
            "\nwarning: {} line(s) did not parse; counts are partial",
            s.skipped_lines
        );
    }
    Ok(())
}

fn sorted(set: &std::collections::BTreeSet<String>) -> Vec<String> {
    set.iter().cloned().collect()
}

fn join(items: &[String]) -> String {
    if items.is_empty() {
        "-".to_string()
    } else {
        items.join(", ")
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max - 1).collect()
    }
}

fn format_mins(secs: i64) -> String {
    if secs < 60 {
        return format!("{secs}s");
    }
    let mins = secs / 60;
    if mins < 60 {
        format!("{mins}m")
    } else {
        format!("{}h{:02}m", mins / 60, mins % 60)
    }
}
