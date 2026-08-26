//! The derive stage and the session index.
//!
//! `live/<host>/projects/` is a verbatim mirror of one host's transcript store
//! and is never written here. Everything under `derived/` is computed from it:
//! facts, events and a report per session, `sessions.json` (the cross-host
//! index — a contract, like the facts), `index.html` (the page a person picks
//! a session from), `state.json` (what was derived from which bytes by which
//! kagviz) and `META.json` (the last run). All of it is regenerable at will,
//! and a kagviz upgrade regenerates all of it, because a changed extractor is
//! changed facts.
//!
//! Two rules carried over from the facts:
//!
//! - **Unchanged means byte-identical.** A session is re-derived when its
//!   source digest or the kagviz that wrote it differs from what `state.json`
//!   recorded — never by mtime, which a re-copy can move without changing a
//!   byte and an appended record can leave alone.
//! - **An absence is visible.** The index carries the sync status the
//!   collector recorded, so a host that did not answer reads as "not reached"
//!   rather than as "nothing new".

use crate::discover::{self, SessionPaths};
use crate::fmt;
use crate::render;
use crate::summary::{self, Involvement, Summary};
use crate::transcript;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The kagviz that produced a derived artifact: crate version and the commit
/// it was built from (`build.rs`). Recorded per session in `state.json`, so a
/// changed extractor re-derives everything it wrote.
pub const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("KAGVIZ_COMMIT"), ")");

/// Where the live mirror lives when neither `--live` nor `$KAGVIZ_LIVE` says.
pub const DEFAULT_LIVE: &str = "/ai-data/kagviz-data/live";

/// The file the collector writes after a sync. Copied into `derived/` so the
/// served tree carries it and the index can say which hosts were reached.
pub const SYNC_STATUS: &str = "sync-status.json";

/// Where `just web-deploy` puts the app, relative to the derived root — the
/// same origin as the data, so the app fetches `../sessions.json` with no CORS
/// and no k-homelab manifest change. `derive` and `index` never write here;
/// the bundle is kagviz-produced and regenerable like everything else under
/// `derived/`, but it is produced by the *build*, not by a run.
pub const APP_ENTRY: &str = "app/index.html";

/// What `state.json` records per `<host>/<session-id>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Derived {
    /// [`source_digest`] of the transcript and its sidecars when derived.
    pub source_digest: String,
    /// [`VERSION`] of the kagviz that derived it.
    pub kagviz: String,
}

pub type State = BTreeMap<String, Derived>;

/// What one run did for one host — the run log, and what `META.json` keeps.
#[derive(Debug, Default, Serialize)]
pub struct HostRun {
    pub sessions: usize,
    pub derived: usize,
    pub unchanged: usize,
    /// Sessions that could not be read. Reported, never silently skipped.
    pub failed: usize,
}

/// The whole run.
#[derive(Debug, Default)]
pub struct Run {
    pub hosts: BTreeMap<String, HostRun>,
    /// Sessions in the regenerated index.
    pub indexed: usize,
}

impl Run {
    pub fn failed(&self) -> usize {
        self.hosts.values().map(|h| h.failed).sum()
    }
}

pub struct Options {
    /// Re-derive every session, even ones whose bytes and kagviz are unchanged.
    pub force: bool,
}

/// The hosts under a live root: every directory holding a `projects/`
/// subdirectory, by name. `derived/` has none, so it is never taken for one.
pub fn hosts(live: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    let entries =
        std::fs::read_dir(live).with_context(|| format!("reading live root {}", live.display()))?;
    for entry in entries {
        let path = entry?.path();
        let projects = path.join("projects");
        if !projects.is_dir() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            out.push((name.to_string(), projects));
        }
    }
    out.sort();
    Ok(out)
}

/// A digest over everything `summarize` reads for a session: the transcript
/// and each subagent sidecar, each with its name and length, in the order
/// `discover` lists them. Content, not mtime — a resumed session appends and
/// a re-copy touches, and only the first should re-derive.
pub fn source_digest(session: &SessionPaths) -> Result<String> {
    let mut hash = Sha256::new();
    for path in std::iter::once(&session.transcript).chain(&session.subagents) {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        hash.update(name.as_bytes());
        hash.update([0u8]);
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
    }
    Ok(format!("sha256:{}", hex(&hash.finalize())))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn facts_path(out: &Path, host: &str, id: &str) -> PathBuf {
    out.join("facts").join(host).join(format!("{id}.json"))
}

pub fn report_path(out: &Path, host: &str, id: &str) -> PathBuf {
    out.join("reports").join(host).join(format!("{id}.html"))
}

/// The events document beside the facts — the detail tier a front-end fetches
/// on demand, the same bytes `kagviz show --events` prints.
pub fn events_path(out: &Path, host: &str, id: &str) -> PathBuf {
    out.join("events").join(host).join(format!("{id}.json"))
}

/// Derive facts and a report for every new or changed session under `live`,
/// then regenerate the index and `META.json`.
///
/// `label` is applied to each freshly counted summary before it is written —
/// the caller decides whether that means asking a model (`--label`) or
/// nothing at all. It is never in the path that produces a number.
///
/// One unreadable session is a warning and a count in the run, not an abort:
/// the other hosts' work still lands, and the failure is on the record.
pub fn derive(
    live: &Path,
    out: &Path,
    opts: &Options,
    label: &mut dyn FnMut(&mut Summary),
) -> Result<Run> {
    std::fs::create_dir_all(out)
        .with_context(|| format!("creating derived root {}", out.display()))?;
    let state_path = out.join("state.json");
    let mut state = read_state(&state_path)?;
    let mut run = Run::default();

    for (host, projects) in hosts(live)? {
        let mut hr = HostRun::default();
        let sessions = discover::sessions(&projects, None)?;
        hr.sessions = sessions.len();
        for session in &sessions {
            let key = format!("{host}/{}", session.id);
            let paths = Outputs {
                facts: facts_path(out, &host, &session.id),
                events: events_path(out, &host, &session.id),
                report: report_path(out, &host, &session.id),
            };
            match derive_one(session, &paths, &key, &mut state, opts.force, label) {
                Ok(true) => hr.derived += 1,
                Ok(false) => hr.unchanged += 1,
                Err(e) => {
                    hr.failed += 1;
                    eprintln!("warning: {key}: {e:#}");
                }
            }
        }
        // After each host rather than at the end, so a run that dies keeps
        // what it did instead of re-deriving it all next time.
        write_json(&state_path, &state)?;
        run.hosts.insert(host, hr);
    }

    let sync = live.join(SYNC_STATUS);
    if sync.is_file() {
        let raw = std::fs::read(&sync).with_context(|| format!("reading {}", sync.display()))?;
        write_atomic(&out.join(SYNC_STATUS), &raw)?;
    }

    run.indexed = index(out)?;
    write_json(
        &out.join("META.json"),
        &serde_json::json!({
            "kagviz": VERSION,
            "generated": Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "sessions": run.indexed,
            "hosts": run.hosts,
        }),
    )?;
    Ok(run)
}

/// What one session derives to.
struct Outputs {
    facts: PathBuf,
    events: PathBuf,
    report: PathBuf,
}

impl Outputs {
    fn all_present(&self) -> bool {
        self.facts.is_file() && self.events.is_file() && self.report.is_file()
    }
}

/// `Ok(true)` when the session was (re)derived, `Ok(false)` when it was
/// already up to date — same bytes, same kagviz, every output present.
fn derive_one(
    session: &SessionPaths,
    out: &Outputs,
    key: &str,
    state: &mut State,
    force: bool,
    label: &mut dyn FnMut(&mut Summary),
) -> Result<bool> {
    let current = Derived {
        source_digest: source_digest(session)?,
        kagviz: VERSION.to_string(),
    };
    if !force && state.get(key) == Some(&current) && out.all_present() {
        return Ok(false);
    }
    let (t, subagents) = transcript::read_session(session)?;
    let (mut s, events) = summary::summarize_with_events(Some(session), &t, &subagents);
    label(&mut s);
    // The same bytes `kagviz show --json > file` writes, trailing newline
    // included, so a derived facts file diffs clean against a baseline. The
    // events likewise, against `show --events`.
    let mut json = serde_json::to_string_pretty(&s)?;
    json.push('\n');
    write_atomic(&out.facts, json.as_bytes())?;
    let mut json = serde_json::to_string_pretty(&events)?;
    json.push('\n');
    write_atomic(&out.events, json.as_bytes())?;
    write_atomic(&out.report, render::report(&s).as_bytes())?;
    state.insert(key.to_string(), current);
    Ok(true)
}

fn read_state(path: &Path) -> Result<State> {
    if !path.is_file() {
        return Ok(State::new());
    }
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut json = serde_json::to_string_pretty(value)?;
    json.push('\n');
    write_atomic(path, json.as_bytes())
}

/// Write via a temporary file and rename, so a reader — copyparty serving the
/// tree, a browser mid-load — never sees a half-written file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} into place", tmp.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The index: sessions.json and index.html, from the derived tree alone.

/// One row of `sessions.json`. Every figure is copied or summed from that
/// session's facts document; `host` is the mirror it came from and the two
/// trailing fields are what `state.json` recorded about deriving it.
///
/// Optional fields are **absent** when unknown, never `null` — the rule the
/// facts document promises and this newer contract keeps from the start.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    pub host: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended: Option<DateTime<Utc>>,
    pub wall_secs: i64,
    pub active_secs: i64,
    pub user_prompts: usize,
    pub assistant_turns: usize,
    /// `tool_calls` summed over tools; `tool_failures` likewise. The session's
    /// own tier only — delegated work is counted in `delegated_spawns`.
    pub tool_calls: u32,
    pub tool_failures: u32,
    pub files_touched: usize,
    pub lines_added: usize,
    pub lines_deleted: usize,
    pub opaque_edits: usize,
    pub output_tokens: u64,
    pub phases: usize,
    pub delegated_spawns: usize,
    pub skipped_lines: usize,
    pub models: Vec<String>,
    pub cli_versions: Vec<String>,
    /// The first non-empty prompt preview — what the session was opened with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opened_by: Option<String>,
    /// The model-written headline, when the facts carry `labels`. Written,
    /// not counted — the same boundary the facts draw.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    /// Paths relative to the derived root, which is the served root.
    pub facts: String,
    pub report: String,
    /// The events document — the detail tier under `facts`. Added in 009.
    #[serde(default)]
    pub events: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kagviz: Option<String>,
}

/// The document `sessions.json` holds. An object rather than a bare array so
/// a later field can be added without breaking a consumer.
#[derive(Debug, Serialize, Deserialize)]
pub struct Sessions {
    pub sessions: Vec<SessionEntry>,
}

/// What the collector recorded. Read tolerantly: every field optional, so a
/// script-shaped file can never take the index down with it.
#[derive(Debug, Default, Deserialize)]
pub struct SyncStatus {
    #[serde(default)]
    pub ran_at: Option<String>,
    #[serde(default)]
    pub hosts: BTreeMap<String, HostSync>,
}

#[derive(Debug, Default, Deserialize)]
pub struct HostSync {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub transferred: Option<u64>,
    #[serde(default)]
    pub secs: Option<u64>,
    #[serde(default)]
    pub note: Option<String>,
}

/// Regenerate `sessions.json` and `index.html` from the facts under `out`.
/// A pure function of the derived tree: the facts files, `state.json`, and
/// the sync status if the collector left one.
pub fn index(out: &Path) -> Result<usize> {
    let state = read_state(&out.join("state.json"))?;
    let mut entries = Vec::new();
    let facts_root = out.join("facts");
    if facts_root.is_dir() {
        for host_dir in sorted_dir(&facts_root)? {
            if !host_dir.is_dir() {
                continue;
            }
            let Some(host) = host_dir.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            for file in sorted_dir(&host_dir)? {
                if file.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let Some(id) = file.file_stem().and_then(|n| n.to_str()) else {
                    continue;
                };
                let raw = std::fs::read_to_string(&file)
                    .with_context(|| format!("reading {}", file.display()))?;
                let s: Summary = serde_json::from_str(&raw).with_context(|| {
                    format!(
                        "parsing facts {} — not a facts document this kagviz reads; \
                         `kagviz derive --force` rewrites the tree",
                        file.display()
                    )
                })?;
                entries.push(entry(host, id, &s, state.get(&format!("{host}/{id}"))));
            }
        }
    }
    entries.sort_by(|a, b| {
        b.started
            .cmp(&a.started)
            .then_with(|| a.host.cmp(&b.host))
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    if entries.iter().any(|e| e.session_id.is_empty()) {
        bail!(
            "a facts file with an empty name under {}",
            facts_root.display()
        );
    }

    let sync = read_sync(&out.join(SYNC_STATUS));
    let doc = Sessions { sessions: entries };
    write_json(&out.join("sessions.json"), &doc)?;
    // Link the app only when it is actually there. A page that offers a
    // link to a 404 is worse than one that does not mention the app: the
    // reader cannot tell "not deployed" from "broken", which is the same
    // failure the sync line exists to prevent one paragraph up.
    let app = out.join(APP_ENTRY).is_file();
    write_atomic(
        &out.join("index.html"),
        index_html(&doc.sessions, sync.as_ref(), app).as_bytes(),
    )?;
    Ok(doc.sessions.len())
}

fn sorted_dir(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<_>>()?;
    paths.sort();
    Ok(paths)
}

fn read_sync(path: &Path) -> Option<SyncStatus> {
    let raw = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&raw) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!(
                "warning: {} is not readable as sync status ({e}); the index will say so",
                path.display()
            );
            None
        }
    }
}

/// One index row from one facts document. Copies and sums; computes nothing
/// that is not already on the page of that session's report.
pub fn entry(host: &str, id: &str, s: &Summary, d: Option<&Derived>) -> SessionEntry {
    SessionEntry {
        host: host.to_string(),
        session_id: id.to_string(),
        project: s.project.clone(),
        cwd: s.cwd.clone(),
        git_branch: s.git_branch.clone(),
        started: s.started,
        ended: s.ended,
        wall_secs: s.wall_secs,
        active_secs: s.active_secs,
        user_prompts: s.user_prompts,
        assistant_turns: s.assistant_turns,
        tool_calls: s.total_tool_calls(),
        tool_failures: s.total_tool_failures(),
        files_touched: s.changes.files_touched,
        lines_added: s.changes.lines_added,
        lines_deleted: s.changes.lines_deleted,
        opaque_edits: s.changes.opaque_edits,
        output_tokens: s.tokens.output,
        phases: s.phases.len(),
        delegated_spawns: s.delegation.spawns.len(),
        skipped_lines: s.skipped_lines,
        models: s.models.keys().cloned().collect(),
        cli_versions: s.cli_versions.iter().cloned().collect(),
        opened_by: s.user_involvement.iter().find_map(|i| match i {
            Involvement::Prompt { preview, .. } if !preview.is_empty() => Some(preview.clone()),
            _ => None,
        }),
        headline: s.labels.as_ref().map(|l| l.headline.clone()),
        facts: format!("facts/{host}/{id}.json"),
        report: format!("reports/{host}/{id}.html"),
        events: format!("events/{host}/{id}.json"),
        source_digest: d.map(|d| d.source_digest.clone()),
        kagviz: d.map(|d| d.kagviz.clone()),
    }
}

/// The browse page. Self-contained like the report — no scripts, no fetches —
/// and every figure on it is copied from `sessions.json`.
pub fn index_html(sessions: &[SessionEntry], sync: Option<&SyncStatus>, app: bool) -> String {
    use render::esc;
    let mut h = String::with_capacity(64 * 1024 + sessions.len() * 700);
    h.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    h.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    h.push_str("<title>kagviz — sessions</title>\n<style>\n");
    h.push_str(CSS);
    h.push_str("</style>\n</head>\n<body>\n<main>\n<header>\n<h1>Sessions</h1>\n");

    let mut per_host: BTreeMap<&str, usize> = BTreeMap::new();
    for s in sessions {
        *per_host.entry(&s.host).or_default() += 1;
    }
    h.push_str(&format!(
        "<p class=\"meta\">{} session(s) across {} host(s): ",
        sessions.len(),
        per_host.len()
    ));
    let chips: Vec<String> = per_host
        .iter()
        .map(|(host, n)| format!("<span class=\"chip\">{} {n}</span>", esc(host)))
        .collect();
    h.push_str(&chips.join(" "));
    h.push_str("</p>\n");
    if app {
        h.push_str(&format!(
            "<p class=\"meta\"><a class=\"app\" href=\"{APP_ENTRY}\">Open the app</a> \
             — the same sessions, sortable and filterable, with a page per session.</p>\n"
        ));
    }
    sync_line(&mut h, sync);
    h.push_str("</header>\n");

    h.push_str("<table class=\"sessions\">\n<thead><tr>");
    for (th, class) in [
        ("started (UTC)", ""),
        ("host", ""),
        ("project", ""),
        ("what", ""),
        ("active", "n"),
        ("prompts", "n"),
        ("tools", "n"),
        ("files", "n"),
        ("", ""),
    ] {
        if class.is_empty() {
            h.push_str(&format!("<th>{th}</th>"));
        } else {
            h.push_str(&format!("<th class=\"{class}\">{th}</th>"));
        }
    }
    h.push_str("</tr></thead>\n<tbody>\n");
    for s in sessions {
        row(&mut h, s);
    }
    h.push_str("</tbody>\n</table>\n");

    h.push_str("<footer>Generated by kagviz ");
    h.push_str(&esc(VERSION));
    h.push_str(
        ". Every figure is computed from the transcript; a line marked \
         <em>written</em> was written by a model over those figures, and is the \
         only thing here that was not measured. Times are UTC.</footer>\n",
    );
    h.push_str("</main>\n</body>\n</html>\n");
    h
}

/// Which hosts the last sync reached. The point of the line is the host that
/// is *missing* from the mirrors: "not reached" on the page, rather than a
/// count that looks complete.
fn sync_line(h: &mut String, sync: Option<&SyncStatus>) {
    use render::esc;
    h.push_str("<p class=\"sync\"><span class=\"k\">last sync</span> ");
    let Some(st) = sync else {
        h.push_str(
            "no sync status recorded — the collector has not run, or these mirrors were \
             not written by it</p>\n",
        );
        return;
    };
    h.push_str(&esc(st
        .ran_at
        .as_deref()
        .unwrap_or("at an unrecorded time")));
    for (host, hs) in &st.hosts {
        let status = hs.status.as_deref().unwrap_or("unknown");
        let (class, detail) = match status {
            "ok" => (
                "ok",
                match hs.secs {
                    Some(secs) => format!(
                        "{} file(s) in {}",
                        hs.transferred.unwrap_or(0),
                        fmt::duration(secs as i64)
                    ),
                    None => format!("{} file(s)", hs.transferred.unwrap_or(0)),
                },
            ),
            "unreachable" => ("unreachable", "not reached".to_string()),
            other => ("failed", other.to_string()),
        };
        let note = hs.note.as_deref().unwrap_or("");
        h.push_str(&format!(
            " · <span class=\"host {class}\" title=\"{}\">{} — {}</span>",
            esc(note),
            esc(host),
            esc(&detail)
        ));
    }
    h.push_str("</p>\n");
}

fn row(h: &mut String, s: &SessionEntry) {
    use render::esc;
    h.push_str("<tr>");
    h.push_str(&format!(
        "<td class=\"mono\" title=\"{}\">{}</td>",
        esc(&s.session_id),
        match &s.started {
            Some(t) => t.format("%Y-%m-%d %H:%M").to_string(),
            None => "—".to_string(),
        }
    ));
    h.push_str(&format!("<td>{}</td>", esc(&s.host)));
    let project = s.cwd.as_deref().or(s.project.as_deref()).unwrap_or("—");
    h.push_str(&format!(
        "<td class=\"proj mono\" title=\"{}\">{}</td>",
        esc(project),
        esc(project)
    ));
    h.push_str("<td class=\"what\">");
    match (&s.headline, &s.opened_by) {
        (Some(head), _) => h.push_str(&format!("<span class=\"said\">{}</span>", esc(head))),
        (None, Some(open)) => h.push_str(&format!("<span class=\"prompt\">{}</span>", esc(open))),
        (None, None) => h.push_str("<span class=\"prompt\">—</span>"),
    }
    if s.skipped_lines > 0 {
        h.push_str(&format!(
            " <span class=\"warn\" title=\"{} transcript line(s) did not parse; every figure in this row is partial\">partial</span>",
            s.skipped_lines
        ));
    }
    h.push_str("</td>");
    h.push_str(&format!(
        "<td class=\"n\">{}<span class=\"sub\">{} wall</span></td>",
        fmt::duration(s.active_secs),
        fmt::duration(s.wall_secs)
    ));
    h.push_str(&format!("<td class=\"n\">{}</td>", s.user_prompts));
    h.push_str(&format!("<td class=\"n\">{}", s.tool_calls));
    if s.tool_failures > 0 {
        h.push_str(&format!(
            "<span class=\"sub fail\">{} failed</span>",
            s.tool_failures
        ));
    }
    if s.delegated_spawns > 0 {
        h.push_str(&format!(
            "<span class=\"sub\">+{} agent(s)</span>",
            s.delegated_spawns
        ));
    }
    h.push_str("</td>");
    h.push_str(&format!(
        "<td class=\"n\">{} <span class=\"add\">+{}</span>/<span class=\"del\">−{}</span>",
        s.files_touched, s.lines_added, s.lines_deleted
    ));
    if s.opaque_edits > 0 {
        h.push_str(&format!(
            "<span class=\"sub\" title=\"{} call(s) could have changed files and left no readable diff; the deltas are a floor\">{} opaque</span>",
            s.opaque_edits, s.opaque_edits
        ));
    }
    h.push_str("</td>");
    h.push_str(&format!(
        "<td class=\"links\"><a href=\"{}\">report</a> · <a href=\"{}\">facts</a> · \
         <a href=\"{}\">events</a></td>",
        esc(&s.report),
        esc(&s.facts),
        esc(&s.events)
    ));
    h.push_str("</tr>\n");
}

/// The report's palette, and only the rules this page needs. Copied rather
/// than shared so the report's bytes stay exactly what the baselines pinned.
const CSS: &str = r#"
:root{
  --bg:#faf9f7; --panel:#fff; --ink:#1b1a18; --muted:#6d6a63; --line:#e4e0d9;
  --accent:#35618f; --fail:#b8412f; --add:#2f7d4f; --del:#b8412f;
  --ask:#a06a1f; --chip:#efece6; --said:#7a4fa0;
}
@media (prefers-color-scheme: dark){
  :root{
    --bg:#16181a; --panel:#1e2124; --ink:#e6e3dd; --muted:#9a958c; --line:#2f3439;
    --accent:#7fb0e0; --fail:#e07a63; --add:#68b884; --del:#e07a63;
    --ask:#d9a34e; --chip:#2a2e33; --said:#b998d8;
  }
}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--ink);
  font:14px/1.45 ui-sans-serif,system-ui,-apple-system,"Segoe UI",Roboto,Helvetica,Arial,sans-serif;}
main{max-width:1480px;margin:0 auto;padding:28px 24px 64px}
h1{font-size:24px;margin:0 0 8px;letter-spacing:-.01em}
header{border-bottom:1px solid var(--line);padding-bottom:14px;margin-bottom:16px}
.meta{margin:0 0 8px;color:var(--muted)}
.chip{display:inline-block;background:var(--chip);border-radius:999px;padding:1px 9px;
  font-size:12px;color:var(--ink)}
a.app{font-weight:600}
.sync{margin:0;font-size:13px;color:var(--muted)}
.sync .k{font-size:11px;text-transform:uppercase;letter-spacing:.08em;margin-right:4px}
.sync .host{padding:1px 8px;border-radius:999px;background:var(--chip);color:var(--ink)}
.sync .host.unreachable{background:transparent;border:1px solid var(--ask);color:var(--ask)}
.sync .host.failed{background:transparent;border:1px solid var(--fail);color:var(--fail)}
table.sessions{width:100%;border-collapse:separate;border-spacing:0;background:var(--panel);
  border:1px solid var(--line);border-radius:10px}
.sessions th{position:sticky;top:0;background:var(--panel);text-align:left;font-size:11px;
  text-transform:uppercase;letter-spacing:.08em;color:var(--muted);font-weight:600;
  padding:10px;border-bottom:1px solid var(--line)}
.sessions td{padding:7px 10px;border-bottom:1px solid var(--line);vertical-align:top}
.sessions tr:last-child td{border-bottom:0}
.sessions .n{text-align:right;white-space:nowrap;font-variant-numeric:tabular-nums}
.mono{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:12.5px;
  white-space:nowrap}
.proj{max-width:28ch;overflow:hidden;text-overflow:ellipsis}
.what{max-width:56ch;min-width:24ch}
.what .prompt{color:var(--muted)}
/* Written, not counted: the same italic serif and accent the report uses for
   its headline, with the word on it, so a sentence in this column can never
   be taken for a measurement. */
.what .said{font:italic 14.5px/1.4 ui-serif,Georgia,"Times New Roman",serif;color:var(--ink)}
.what .said::before{content:"written ";font:600 10px/1 ui-sans-serif,system-ui,sans-serif;
  letter-spacing:.08em;text-transform:uppercase;color:var(--said);margin-right:4px}
.sub{display:block;font-size:11.5px;color:var(--muted);font-weight:400}
.fail{color:var(--fail)} .add{color:var(--add)} .del{color:var(--del)}
.warn{color:var(--ask);font-size:11px;text-transform:uppercase;letter-spacing:.06em}
.links{white-space:nowrap}
a{color:var(--accent)}
footer{margin-top:24px;font-size:12px;color:var(--muted)}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A scratch tree per test, under the target dir so it is git-ignored and
    /// `cargo clean` removes it.
    fn scratch(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("derive-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const PROMPT: &str = r#"{"type":"user","timestamp":"2026-08-20T10:00:00.000Z","sessionId":"s1","cwd":"/home/ken/src/x","message":{"content":"make it go"}}"#;
    const TOOL: &str = r#"{"type":"assistant","timestamp":"2026-08-20T10:00:30.000Z","message":{"model":"claude-opus-5","usage":{"output_tokens":40},"content":[{"type":"tool_use","id":"t1","name":"Read"}]}}"#;
    const LATER: &str = r#"{"type":"user","timestamp":"2026-08-20T10:01:00.000Z","message":{"content":[{"type":"tool_result","tool_use_id":"t1"}]}}"#;

    fn mirror(live: &Path, host: &str, id: &str, lines: &[&str]) -> PathBuf {
        let dir = live.join(host).join("projects").join("-home-ken-src-x");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{id}.jsonl"));
        fs::write(&path, lines.join("\n") + "\n").unwrap();
        path
    }

    fn no_labels(_: &mut Summary) {}

    #[test]
    fn a_host_is_a_directory_with_projects_under_it() {
        let live = scratch("hosts");
        mirror(&live, "kai", "a", &[PROMPT]);
        fs::create_dir_all(live.join("derived").join("facts")).unwrap();
        fs::create_dir_all(live.join("notes")).unwrap();
        let names: Vec<String> = hosts(&live).unwrap().into_iter().map(|(h, _)| h).collect();
        assert_eq!(names, ["kai"]);
    }

    /// Content, not mtime: an appended record changes the digest, a sidecar
    /// appearing changes it, and rewriting identical bytes does not.
    #[test]
    fn the_source_digest_follows_bytes_not_timestamps() {
        let live = scratch("digest");
        let path = mirror(&live, "kai", "a", &[PROMPT, TOOL]);
        let session = || {
            discover::sessions(&live.join("kai").join("projects"), None)
                .unwrap()
                .remove(0)
        };
        let d1 = source_digest(&session()).unwrap();
        assert!(d1.starts_with("sha256:"));

        let same = fs::read(&path).unwrap();
        fs::write(&path, &same).unwrap();
        assert_eq!(
            source_digest(&session()).unwrap(),
            d1,
            "a rewrite is not a change"
        );

        fs::write(&path, [PROMPT, TOOL, LATER].join("\n") + "\n").unwrap();
        let d2 = source_digest(&session()).unwrap();
        assert_ne!(d2, d1, "an appended record is a change");

        let sidecar = path.with_extension("").join("subagents");
        fs::create_dir_all(&sidecar).unwrap();
        fs::write(sidecar.join("agent-x.jsonl"), format!("{TOOL}\n")).unwrap();
        assert_ne!(
            source_digest(&session()).unwrap(),
            d2,
            "a new sidecar is a change"
        );
    }

    #[test]
    fn derive_writes_once_and_again_only_when_the_bytes_change() {
        let live = scratch("derive");
        let path = mirror(&live, "kai", "a", &[PROMPT, TOOL]);
        mirror(&live, "kubs0", "b", &[PROMPT]);
        let out = live.join("derived");
        let opts = Options { force: false };

        let run = derive(&live, &out, &opts, &mut no_labels).unwrap();
        assert_eq!(run.hosts["kai"].derived, 1);
        assert_eq!(run.hosts["kubs0"].derived, 1);
        assert_eq!(run.indexed, 2);
        assert!(facts_path(&out, "kai", "a").is_file());
        assert!(events_path(&out, "kai", "a").is_file());
        assert!(report_path(&out, "kai", "a").is_file());
        assert!(out.join("sessions.json").is_file());
        assert!(out.join("index.html").is_file());
        assert!(out.join("META.json").is_file());
        let state = read_state(&out.join("state.json")).unwrap();
        assert_eq!(state["kai/a"].kagviz, VERSION);

        // Nothing moved: nothing re-derived, and the facts bytes are the ones
        // `show --json` would print.
        let facts_before = fs::read(facts_path(&out, "kai", "a")).unwrap();
        let run = derive(&live, &out, &opts, &mut no_labels).unwrap();
        assert_eq!(run.hosts["kai"].derived, 0);
        assert_eq!(run.hosts["kai"].unchanged, 1);
        assert_eq!(
            fs::read(facts_path(&out, "kai", "a")).unwrap(),
            facts_before
        );
        assert!(facts_before.ends_with(b"}\n"));

        // The session was resumed: one host re-derives, the other does not.
        fs::write(&path, [PROMPT, TOOL, LATER].join("\n") + "\n").unwrap();
        let run = derive(&live, &out, &opts, &mut no_labels).unwrap();
        assert_eq!(run.hosts["kai"].derived, 1);
        assert_eq!(run.hosts["kubs0"].unchanged, 1);

        // A derived file that went missing is rebuilt even with nothing changed.
        fs::remove_file(report_path(&out, "kubs0", "b")).unwrap();
        let run = derive(&live, &out, &opts, &mut no_labels).unwrap();
        assert_eq!(run.hosts["kubs0"].derived, 1);

        // --force re-derives everything.
        let run = derive(&live, &out, &Options { force: true }, &mut no_labels).unwrap();
        assert_eq!(run.hosts["kai"].derived + run.hosts["kubs0"].derived, 2);
    }

    /// The label hook runs on every fresh summary and its output lands in the
    /// facts — the derive is where `--label` would be applied nightly.
    #[test]
    fn the_label_hook_sees_each_derived_summary() {
        let live = scratch("label");
        mirror(&live, "kai", "a", &[PROMPT, TOOL]);
        let out = live.join("derived");
        let mut seen = 0;
        derive(&live, &out, &Options { force: false }, &mut |s| {
            seen += 1;
            assert_eq!(s.session_id.as_deref(), Some("s1"));
        })
        .unwrap();
        assert_eq!(seen, 1);
    }

    #[test]
    fn the_index_lists_newest_first_and_says_which_hosts_were_reached() {
        let live = scratch("index");
        mirror(&live, "kai", "old", &[PROMPT, TOOL]);
        mirror(
            &live,
            "cleo",
            "new",
            &[&PROMPT.replace("2026-08-20T10:00:00", "2026-08-21T09:00:00")],
        );
        fs::write(
            live.join(SYNC_STATUS),
            r#"{"ran_at":"2026-08-25T11:00:02Z","hosts":{"kai":{"status":"ok","transferred":3,"secs":1},"cleo":{"status":"unreachable","transferred":0,"secs":0,"note":"did not answer ssh"},"kubs0":{"status":"failed","note":"rsync exit 23"}}}"#,
        )
        .unwrap();
        let out = live.join("derived");
        derive(&live, &out, &Options { force: false }, &mut no_labels).unwrap();

        let doc: Sessions =
            serde_json::from_str(&fs::read_to_string(out.join("sessions.json")).unwrap()).unwrap();
        let order: Vec<(&str, &str)> = doc
            .sessions
            .iter()
            .map(|s| (s.host.as_str(), s.session_id.as_str()))
            .collect();
        assert_eq!(order, [("cleo", "new"), ("kai", "old")]);
        let kai = &doc.sessions[1];
        assert_eq!(kai.report, "reports/kai/old.html");
        assert_eq!(kai.facts, "facts/kai/old.json");
        assert_eq!(kai.events, "events/kai/old.json");
        assert!(
            out.join(&kai.events).is_file(),
            "the link on the page resolves"
        );
        assert_eq!(kai.tool_calls, 1);
        assert_eq!(kai.opened_by.as_deref(), Some("make it go"));
        assert_eq!(kai.kagviz.as_deref(), Some(VERSION));
        assert!(kai.source_digest.as_deref().unwrap().starts_with("sha256:"));

        // Absent, never null: no headline was written and no git branch recorded.
        let raw = fs::read_to_string(out.join("sessions.json")).unwrap();
        assert!(!raw.contains("null"), "sessions.json emits null: {raw}");
        assert!(!raw.contains("\"headline\""));

        let html = fs::read_to_string(out.join("index.html")).unwrap();
        assert!(html.contains("kai — 3 file(s) in 1s"));
        assert!(html.contains("cleo — not reached"));
        assert!(html.contains("host failed"), "a failed host is marked");
        assert!(html.contains("did not answer ssh"));
        let new_at = html.find("reports/cleo/new.html").unwrap();
        let old_at = html.find("reports/kai/old.html").unwrap();
        assert!(new_at < old_at, "newest session first");
        assert!(html.contains("make it go"));
        assert!(
            out.join(SYNC_STATUS).is_file(),
            "the status is copied into the served tree"
        );
    }

    #[test]
    fn a_headline_on_the_index_is_marked_written() {
        let live = scratch("headline");
        mirror(&live, "kai", "a", &[PROMPT, TOOL]);
        let out = live.join("derived");
        derive(&live, &out, &Options { force: false }, &mut |s| {
            s.labels = Some(crate::label::Labels {
                headline: "Read one file and stopped.".to_string(),
                phases: vec![],
                model: "test-model".to_string(),
                prompt_version: "headline.v1".to_string(),
                facts_digest: "sha256:0".to_string(),
                generated: Utc::now(),
            });
        })
        .unwrap();
        let html = fs::read_to_string(out.join("index.html")).unwrap();
        assert!(html.contains("<span class=\"said\">Read one file and stopped.</span>"));
        let doc: Sessions =
            serde_json::from_str(&fs::read_to_string(out.join("sessions.json")).unwrap()).unwrap();
        assert_eq!(
            doc.sessions[0].headline.as_deref(),
            Some("Read one file and stopped.")
        );
    }

    /// The app is linked when it is there and not mentioned when it is not.
    ///
    /// A link to a 404 would leave the reader unable to tell "not deployed"
    /// from "broken" — the same distinction the sync line above it exists to
    /// keep visible.
    #[test]
    fn the_index_links_the_app_only_once_it_is_deployed() {
        let live = scratch("applink");
        mirror(&live, "kai", "a", &[PROMPT, TOOL]);
        let out = live.join("derived");

        derive(&live, &out, &Options { force: false }, &mut no_labels).unwrap();
        let before = fs::read_to_string(out.join("index.html")).unwrap();
        assert!(
            !before.contains(APP_ENTRY),
            "linked an app that is not there"
        );

        fs::create_dir_all(out.join("app")).unwrap();
        fs::write(out.join(APP_ENTRY), "<!doctype html>").unwrap();
        index(&out).unwrap();
        let after = fs::read_to_string(out.join("index.html")).unwrap();
        assert!(after.contains(&format!("href=\"{APP_ENTRY}\"")), "{after}");
        assert!(after.contains("Open the app"));
    }

    /// Same bar as the report: nothing on the page can fetch anything.
    #[test]
    fn the_index_is_self_contained() {
        let live = scratch("selfcontained");
        mirror(
            &live,
            "kai",
            "a",
            &[&PROMPT.replace("make it go", "see https://example.invalid/spec")],
        );
        let out = live.join("derived");
        derive(&live, &out, &Options { force: false }, &mut no_labels).unwrap();
        let html = fs::read_to_string(out.join("index.html")).unwrap();
        assert!(html.starts_with("<!doctype html>"));
        for probe in [
            "<script", "<link", "<img", "<iframe", "<object", "<embed", "src=", "@import", "url(",
        ] {
            assert!(!html.contains(probe), "index can fetch: {probe}");
        }
        assert!(html.contains("https://example.invalid/spec"));
    }

    #[test]
    fn an_empty_derived_tree_still_yields_an_index() {
        let live = scratch("empty");
        let out = live.join("derived");
        fs::create_dir_all(&out).unwrap();
        assert_eq!(index(&out).unwrap(), 0);
        let raw = fs::read_to_string(out.join("sessions.json")).unwrap();
        assert_eq!(raw, "{\n  \"sessions\": []\n}\n");
        assert!(
            fs::read_to_string(out.join("index.html"))
                .unwrap()
                .contains("no sync status recorded")
        );
    }
}
