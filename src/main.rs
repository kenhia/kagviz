//! kagviz — deterministic insight into how an agent session went.
//!
//! Reads Claude Code session transcripts and reports what actually happened.
//! Every number is derived from the transcript bytes, never inferred.

mod discover;
mod fmt;
mod render;
mod summary;
mod transcript;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use summary::Summary;
use transcript::{Subagent, Transcript};

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
    /// Render a session as a self-contained HTML report.
    Render {
        /// Session id. Omit when reading facts with --from.
        session_id: Option<String>,
        /// Render a facts document written by `show --json` (`-` for stdin)
        /// instead of reading a transcript.
        #[arg(long, conflicts_with = "session_id", value_name = "FACTS.json")]
        from: Option<PathBuf>,
        /// Write the report here (default: stdout).
        #[arg(short, long, value_name = "REPORT.html")]
        out: Option<PathBuf>,
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
        Command::Render {
            session_id,
            from,
            out,
        } => render_report(
            &root,
            session_id.as_deref(),
            from.as_deref(),
            out.as_deref(),
        ),
    }
}

fn list_sessions(root: &Path, project: Option<&str>) -> Result<()> {
    let sessions = discover::sessions(root, project)?;
    println!(
        "{:<38} {:<22} {:>7} {:>7} {:>6}",
        "SESSION", "PROJECT", "ACTIVE", "TOOLS", "FAIL"
    );
    for session in &sessions {
        let (t, subagents) = read_session(session)?;
        let s = summary::summarize(Some(session), &t, &subagents);
        println!(
            "{:<38} {:<22} {:>7} {:>7} {:>6}",
            session.id,
            truncate(&session.project, 22),
            fmt::duration(s.active_secs),
            s.total_tool_calls(),
            s.total_tool_failures(),
        );
    }
    eprintln!("\n{} session(s) under {}", sessions.len(), root.display());
    Ok(())
}

/// Read a session's transcript and every subagent sidecar beside it.
///
/// The sidecars are read here, at the edge, so `summarize` stays a pure
/// function of bytes handed to it rather than of what happens to be on disk.
fn read_session(session: &discover::SessionPaths) -> Result<(Transcript, Vec<Subagent>)> {
    let t = transcript::read(&session.transcript)?;
    let subagents = session
        .subagents
        .iter()
        .map(|p| transcript::read_subagent(p))
        .collect::<Result<Vec<_>>>()?;
    Ok((t, subagents))
}

/// Summarize one session from its transcript on disk.
fn load_session(root: &Path, id: &str) -> Result<Summary> {
    let session = discover::sessions(root, None)?
        .into_iter()
        .find(|s| s.id == id)
        .with_context(|| format!("no session {id} under {}", root.display()))?;
    let (t, subagents) = read_session(&session)?;
    Ok(summary::summarize(Some(&session), &t, &subagents))
}

/// Read a facts document written by `show --json` (`-` reads stdin).
///
/// This is the seam the renderer is built on: whatever consumes the facts —
/// this binary today, a front-end later — reads the same document.
fn load_facts(path: &Path) -> Result<Summary> {
    let raw = if path == Path::new("-") {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading facts from stdin")?;
        buf
    } else {
        std::fs::read_to_string(path)
            .with_context(|| format!("reading facts {}", path.display()))?
    };
    serde_json::from_str(&raw).with_context(|| format!("parsing facts {}", path.display()))
}

fn render_report(
    root: &Path,
    id: Option<&str>,
    from: Option<&Path>,
    out: Option<&Path>,
) -> Result<()> {
    let summary = match (id, from) {
        (_, Some(path)) => load_facts(path)?,
        (Some(id), None) => load_session(root, id)?,
        (None, None) => bail!("give a session id, or --from <facts.json>"),
    };
    let html = render::report(&summary);
    match out {
        Some(path) => {
            std::fs::write(path, &html)
                .with_context(|| format!("writing report {}", path.display()))?;
            eprintln!("wrote {} ({} bytes)", path.display(), html.len());
        }
        None => print!("{html}"),
    }
    Ok(())
}

fn show_session(root: &Path, id: &str, json: bool) -> Result<()> {
    let s = load_session(root, id)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&s)?);
        return Ok(());
    }

    println!("session   {}", s.session_id.as_deref().unwrap_or(id));
    println!("project   {}", s.project.as_deref().unwrap_or("-"));
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
        fmt::duration(s.active_secs),
        fmt::duration(s.wall_secs),
        fmt::duration(s.idle_secs)
    );
    if let Some(dominant) = s.dominant_phase() {
        println!("phases    {} (mostly {})", s.phases.len(), dominant.label());
        for (kind, n, secs) in s.phase_rollup() {
            println!(
                "            {n:>4}  {:<13} {}",
                kind.label(),
                fmt::duration(secs)
            );
        }
    }
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
        "files     {} touched, +{}/-{}  [{} opaque call(s) unaccounted]",
        s.changes.files_touched,
        s.changes.lines_added,
        s.changes.lines_deleted,
        s.changes.opaque_edits
    );
    for (tool, c) in &s.changes.by_tool {
        let seen = if c.opaque == c.calls {
            format!("{} unreadable", c.opaque)
        } else {
            format!(
                "{} file(s) +{}/-{}{}",
                c.files_touched,
                c.lines_added,
                c.lines_deleted,
                if c.opaque > 0 {
                    format!(", {} unreadable", c.opaque)
                } else {
                    String::new()
                }
            )
        };
        println!("            {:>4}  {tool}  ({seen})", c.calls);
    }
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
    print_delegation(&s);
    if s.skipped_lines > 0 {
        eprintln!(
            "\nwarning: {} line(s) did not parse; counts are partial",
            s.skipped_lines
        );
    }
    Ok(())
}

/// Delegated work as its own tier, with the combined line spelled out.
///
/// The combined figures are the point of printing this at all: a session that
/// spawned two agents shows two `Agent` calls in the tool list above, and the
/// real cost is down here.
fn print_delegation(s: &Summary) {
    let d = &s.delegation;
    if d.is_empty() {
        return;
    }
    println!(
        "delegated {} agent(s), {} tool call(s), {} out",
        d.spawns.len(),
        d.totals.tool_calls.values().sum::<u32>(),
        fmt::count(d.totals.tokens.output),
    );
    for spawn in &d.spawns {
        println!(
            "            {:<10} {:>4} call(s)  {:>7}  {:>8} out  {}",
            spawn.subagent_type.as_deref().unwrap_or("agent"),
            spawn.tool_calls.values().sum::<u32>(),
            fmt::duration(spawn.active_secs),
            fmt::count(spawn.tokens.output),
            spawn.description.as_deref().unwrap_or(""),
        );
    }
    if d.unjoined_spawns > 0 {
        println!(
            "            {} spawn(s) have no transcript; their work is not counted here",
            d.unjoined_spawns
        );
    }
    if d.inline_records > 0 {
        println!(
            "            {} record(s) were subagent turns inlined in this transcript",
            d.inline_records
        );
    }
    println!(
        "combined  {} tool call(s), {} failed, {} out  (session + delegated)",
        s.combined_tool_calls(),
        s.combined_tool_failures(),
        fmt::count(s.combined_output_tokens()),
    );
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
