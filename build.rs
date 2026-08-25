//! Stamp the binary with the commit it was built from.
//!
//! Everything under `derived/` says which kagviz produced it (`state.json`,
//! `META.json`), and a session is re-derived when that changes — a changed
//! extractor is changed facts. `unknown` outside a git checkout rather than a
//! build failure, so a tarball build still works and still says so.
//!
//! A dirty working tree is stamped with its base commit: a build script only
//! re-runs when its declared inputs change, so a `-dirty` suffix would go stale
//! the moment a source file was edited without a commit.

use std::process::Command;

fn main() {
    let commit = Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=KAGVIZ_COMMIT={commit}");

    // HEAD names a ref; a commit on the branch moves the ref, not HEAD.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/packed-refs");
    if let Ok(head) = std::fs::read_to_string(".git/HEAD")
        && let Some(r) = head.trim().strip_prefix("ref: ")
    {
        println!("cargo:rerun-if-changed=.git/{r}");
    }
}
