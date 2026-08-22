//! Locating transcripts on disk.
//!
//! Claude Code stores sessions under `<home>/.claude/projects/<project-slug>/`,
//! where the slug is the working directory with separators flattened. Each
//! session is `<session-id>.jsonl`, optionally beside a `<session-id>/`
//! directory holding `subagents/` transcripts and `tool-results/` overflow.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// A session transcript found on disk, with its sidecars.
#[derive(Debug, Clone)]
pub struct SessionPaths {
    pub id: String,
    pub project: String,
    pub transcript: PathBuf,
    /// Subagent transcripts, if the session spawned any.
    pub subagents: Vec<PathBuf>,
}

/// The default transcript root: `<home>/.claude/projects`.
///
/// Resolved from `HOME`, falling back to `USERPROFILE` so the same binary
/// works on Windows, where a good share of these transcripts live.
pub fn default_root() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .context("neither HOME nor USERPROFILE is set; pass --root explicitly")?;
    Ok(Path::new(&home).join(".claude").join("projects"))
}

/// Enumerate every session under `root`, optionally narrowed to one project
/// slug. Sorted by project then session id so output is stable run to run.
pub fn sessions(root: &Path, project: Option<&str>) -> Result<Vec<SessionPaths>> {
    let mut found = Vec::new();

    let entries =
        std::fs::read_dir(root).with_context(|| format!("reading root {}", root.display()))?;
    for project_dir in entries {
        let project_dir = project_dir?.path();
        if !project_dir.is_dir() {
            continue;
        }
        let slug = match project_dir.file_name().and_then(|n| n.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        if project.is_some_and(|want| want != slug) {
            continue;
        }

        for entry in std::fs::read_dir(&project_dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let id = match path.file_stem().and_then(|n| n.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let subagents = subagent_transcripts(&project_dir.join(&id));
            found.push(SessionPaths {
                id,
                project: slug.clone(),
                transcript: path,
                subagents,
            });
        }
    }

    found.sort_by(|a, b| (&a.project, &a.id).cmp(&(&b.project, &b.id)));
    Ok(found)
}

/// Subagent transcripts beside a session, if its sidecar directory exists.
///
/// A missing or unreadable sidecar is simply "no subagents" — older CLI
/// versions inlined subagent turns into the main transcript instead.
fn subagent_transcripts(sidecar: &Path) -> Vec<PathBuf> {
    let dir = sidecar.join("subagents");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
        .collect();
    paths.sort();
    paths
}
