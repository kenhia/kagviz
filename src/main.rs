//! kagviz — deterministic insight into how an agent session went.
//!
//! Reads Claude Code session transcripts and reports what actually happened.
//! Every number is derived from the transcript bytes, never inferred.

mod derive;
mod discover;
mod fmt;
mod label;
mod render;
mod summary;
mod transcript;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use clap::{Args, Parser, Subcommand};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use summary::Summary;

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
        #[command(flatten)]
        label: LabelOpts,
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
        #[command(flatten)]
        label: LabelOpts,
    },
    /// Derive facts, reports and the session index from the live mirrors.
    ///
    /// Reads `<live>/<host>/projects/` for every host, writes `<live>/derived/`.
    /// Only sessions whose bytes or kagviz changed are re-derived.
    Derive {
        /// The live mirror root (default: $KAGVIZ_LIVE, else /ai-data/kagviz-data/live).
        #[arg(long, value_name = "DIR")]
        live: Option<PathBuf>,
        /// Where derived artifacts go (default: <live>/derived).
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
        /// Re-derive every session, even ones whose bytes and kagviz are unchanged.
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        label: LabelOpts,
    },
    /// Regenerate sessions.json and index.html from an existing derived tree.
    Index {
        /// The derived tree (default: $KAGVIZ_LIVE/derived, else /ai-data/kagviz-data/live/derived).
        #[arg(value_name = "DIR")]
        derived: Option<PathBuf>,
    },
}

/// The opt-in headline pass. Off by default, and that is the contract: a
/// plain `render` is a pure function of the transcript bytes.
#[derive(Args, Debug, Default, Clone)]
struct LabelOpts {
    /// Ask a model to write a headline and a label per phase over the facts.
    #[arg(long)]
    label: bool,
    /// Ignore cached labels and ask the model again.
    #[arg(long, requires = "label")]
    relabel: bool,
    /// OpenAI-compatible base URL (default: $KVLLM_BASE_URL, else localhost).
    #[arg(long, value_name = "URL", requires = "label")]
    label_url: Option<String>,
    /// Model to label with. `auto` asks the backend what it serves.
    #[arg(long, value_name = "MODEL", default_value = "auto", requires = "label")]
    label_model: String,
    /// Where cached labels live (default: <root>/.kagviz/labels).
    #[arg(long, value_name = "DIR", requires = "label")]
    label_cache: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = match cli.root {
        Some(r) => r,
        None => discover::default_root()?,
    };

    match cli.command {
        Command::Sessions { project } => list_sessions(&root, project.as_deref()),
        Command::Show {
            session_id,
            json,
            label,
        } => show_session(&root, &session_id, json, &label),
        Command::Render {
            session_id,
            from,
            out,
            label,
        } => render_report(
            &root,
            session_id.as_deref(),
            from.as_deref(),
            out.as_deref(),
            &label,
        ),
        Command::Derive {
            live,
            out,
            force,
            label,
        } => derive_sessions(live.as_deref(), out.as_deref(), force, &label),
        Command::Index { derived } => index_sessions(derived.as_deref()),
    }
}

/// The live mirror root: the flag, else the environment, else the homelab's
/// data volume.
fn live_root(flag: Option<&Path>) -> PathBuf {
    flag.map(Path::to_path_buf)
        .or_else(|| std::env::var_os("KAGVIZ_LIVE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(derive::DEFAULT_LIVE))
}

fn derive_sessions(
    live: Option<&Path>,
    out: Option<&Path>,
    force: bool,
    label: &LabelOpts,
) -> Result<()> {
    let live = live_root(live);
    let out = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| live.join("derived"));
    // Labels cache under derived/, never inside a mirror: the mirrors are
    // verbatim copies of what the harness wrote and stay that way.
    let label = LabelOpts {
        label_cache: Some(
            label
                .label_cache
                .clone()
                .unwrap_or_else(|| out.join("labels")),
        ),
        ..label.clone()
    };
    let started = std::time::Instant::now();
    let run = derive::derive(&live, &out, &derive::Options { force }, &mut |s| {
        label_facts(s, &live, &label)
    })?;
    for (host, r) in &run.hosts {
        println!(
            "{host:<8} {:>5} session(s)  {:>5} derived  {:>5} unchanged{}",
            r.sessions,
            r.derived,
            r.unchanged,
            if r.failed > 0 {
                format!("  {} FAILED", r.failed)
            } else {
                String::new()
            }
        );
    }
    if run.hosts.is_empty() {
        println!(
            "no hosts under {} (a host is a directory holding projects/)",
            live.display()
        );
    }
    println!(
        "index    {:>5} session(s) → {}",
        run.indexed,
        out.join("index.html").display()
    );
    println!(
        "kagviz   {}  in {:.1}s",
        derive::VERSION,
        started.elapsed().as_secs_f64()
    );
    if run.failed() > 0 {
        bail!(
            "{} session(s) could not be derived; see the warnings above",
            run.failed()
        );
    }
    Ok(())
}

fn index_sessions(derived: Option<&Path>) -> Result<()> {
    let derived = derived
        .map(Path::to_path_buf)
        .unwrap_or_else(|| live_root(None).join("derived"));
    let n = derive::index(&derived)?;
    println!(
        "index    {n:>5} session(s) → {}",
        derived.join("index.html").display()
    );
    Ok(())
}

/// Attach model-written labels to the facts, if they were asked for.
///
/// Failure here is a **warning, not an error**. Making a report fail because
/// the model backend is unreachable would put a model in the path that
/// produces the deterministic page — the exact inversion the labels are
/// sandboxed to prevent. The reader loses the headline and is told why.
fn label_facts(s: &mut Summary, root: &Path, opts: &LabelOpts) {
    if !opts.label {
        return;
    }
    if let Err(e) = attach_labels(s, root, opts) {
        eprintln!("warning: no headline written — {e:#}");
    }
}

fn attach_labels(s: &mut Summary, root: &Path, opts: &LabelOpts) -> Result<()> {
    let digest = label::facts_digest(s)?;
    let dir = opts
        .label_cache
        .clone()
        .unwrap_or_else(|| label::default_cache_dir(root));

    // The cache is consulted before the backend is even resolved, so a report
    // whose labels are already written re-renders with the model host off.
    let want_model = (opts.label_model != "auto").then_some(opts.label_model.as_str());
    if !opts.relabel
        && let Some(hit) = label::cached(&dir, &digest, want_model)
    {
        s.labels = Some(hit);
        return Ok(());
    }

    let url = opts
        .label_url
        .clone()
        .or_else(|| std::env::var("KVLLM_BASE_URL").ok())
        .unwrap_or_else(|| label::DEFAULT_BASE_URL.to_string());
    let backend = label::Kvllm::connect(&url, &opts.label_model)?;
    let labels = label::generate(s, &backend, Utc::now())?;
    label::store(&dir, &labels)?;
    eprintln!("labelled by {}", label::attribution(&labels));
    s.labels = Some(labels);
    Ok(())
}

fn list_sessions(root: &Path, project: Option<&str>) -> Result<()> {
    let sessions = discover::sessions(root, project)?;
    println!(
        "{:<38} {:<22} {:>7} {:>7} {:>6}",
        "SESSION", "PROJECT", "ACTIVE", "TOOLS", "FAIL"
    );
    for session in &sessions {
        let (t, subagents) = transcript::read_session(session)?;
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

/// Summarize one session from its transcript on disk.
fn load_session(root: &Path, id: &str) -> Result<Summary> {
    let session = discover::sessions(root, None)?
        .into_iter()
        .find(|s| s.id == id)
        .with_context(|| format!("no session {id} under {}", root.display()))?;
    let (t, subagents) = transcript::read_session(&session)?;
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
    label: &LabelOpts,
) -> Result<()> {
    let mut summary = match (id, from) {
        (_, Some(path)) => load_facts(path)?,
        (Some(id), None) => load_session(root, id)?,
        (None, None) => bail!("give a session id, or --from <facts.json>"),
    };
    // A facts document that already carries labels renders them without any
    // model call: `--label` governs *writing* them, never showing them.
    if summary.labels.is_none() {
        label_facts(&mut summary, root, label);
    }
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

fn show_session(root: &Path, id: &str, json: bool, label: &LabelOpts) -> Result<()> {
    let mut s = load_session(root, id)?;
    label_facts(&mut s, root, label);
    let s = s;

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
    let rate = s
        .tool_failure_rate()
        .map(|r| format!(" ({})", fmt::percent(r)))
        .unwrap_or_default();
    println!(
        "tools     {} calls, {} failed{rate}",
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
    print_labels(&s);
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

/// The model-written block, last and clearly fenced.
///
/// Last rather than first on purpose: in a terminal the counts are the answer
/// and the sentence is the gloss. The `written` prefix is on every line so no
/// single line of this output can be mistaken for a measurement when it is
/// grepped, piped or pasted out of context.
fn print_labels(s: &Summary) {
    let Some(l) = &s.labels else {
        return;
    };
    println!("\nwritten   {}", l.headline);
    for p in &l.phases {
        println!("written     phase {:<3} {}", p.phase + 1, p.label);
    }
    println!(
        "written   ^ written by {} over the facts above — not measured",
        label::attribution(l)
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
