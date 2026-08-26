//! The golden render: the in-repo fixture, through the built binary, byte
//! for byte.
//!
//! Everything the corpus sweeps prove against `/ai-data` is unreproducible
//! from a clone; this is the part a clone *can* prove. One hand-written
//! session (`tests/fixtures/README.md` says what it exercises), every
//! presentation layer kagviz has — the facts, the events, the report, the
//! `sessions` table and the terminal `show` — compared to what is checked in.
//!
//! Driven through `CARGO_BIN_EXE_kagviz`, the real binary, so `discover`,
//! `load_facts` and the CLI wiring are on the path rather than only the
//! library functions behind them.
//!
//! When a change moves a golden on purpose:
//!
//! ```sh
//! KAGVIZ_UPDATE_GOLDEN=1 cargo test --test golden
//! git diff tests/golden/
//! ```
//!
//! The diff is the review surface: a moved number shows up as a moved
//! number, a reworded page as a reworded page, and both show up.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn root() -> PathBuf {
    manifest().join("tests").join("fixtures").join("root")
}

fn golden(name: &str) -> PathBuf {
    manifest().join("tests").join("golden").join(name)
}

const SESSION: &str = "fixture-0001";

/// Run the built binary against the fixture root.
fn kagviz(args: &[&str]) -> (String, String, bool) {
    kagviz_at(&root(), args, None)
}

fn kagviz_at(root: &Path, args: &[&str], stdin: Option<&str>) -> (String, String, bool) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kagviz"));
    cmd.arg("--root").arg(root).args(args);
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("kagviz starts");
    if let Some(input) = stdin {
        use std::io::Write as _;
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(input.as_bytes())
            .expect("stdin written");
    }
    let out = child.wait_with_output().expect("kagviz finishes");
    (
        String::from_utf8(out.stdout).expect("utf-8 stdout"),
        String::from_utf8(out.stderr).expect("utf-8 stderr"),
        out.status.success(),
    )
}

/// Compare `actual` to the golden, or rewrite the golden when asked to.
fn check(name: &str, actual: &str) {
    let path = golden(name);
    if std::env::var_os("KAGVIZ_UPDATE_GOLDEN").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, actual).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "no golden at {}: {e}\nKAGVIZ_UPDATE_GOLDEN=1 cargo test --test golden writes it",
            path.display()
        )
    });
    if expected != actual {
        let line = expected
            .lines()
            .zip(actual.lines())
            .position(|(e, a)| e != a)
            .map_or(
                expected.lines().count().min(actual.lines().count()) + 1,
                |i| i + 1,
            );
        let (want, got) = (
            expected.lines().nth(line - 1).unwrap_or("<end>"),
            actual.lines().nth(line - 1).unwrap_or("<end>"),
        );
        panic!(
            "{name} differs from tests/golden/{name} at line {line}:\n  golden: {want}\n  actual: {got}\n\
             KAGVIZ_UPDATE_GOLDEN=1 cargo test --test golden rewrites it — then read the diff."
        );
    }
}

#[test]
fn the_facts_are_golden() {
    let (out, err, ok) = kagviz(&["show", SESSION, "--json"]);
    assert!(ok, "{err}");
    check("fixture-0001.facts.json", &out);
}

#[test]
fn the_events_are_golden() {
    let (out, err, ok) = kagviz(&["show", SESSION, "--events"]);
    assert!(ok, "{err}");
    check("fixture-0001.events.json", &out);
}

#[test]
fn the_report_is_golden() {
    let (out, err, ok) = kagviz(&["render", SESSION]);
    assert!(ok, "{err}");
    check("fixture-0001.report.html", &out);
}

/// The third presentation layer, the one that gets forgotten.
#[test]
fn the_terminal_show_is_golden() {
    let (out, err, ok) = kagviz(&["show", SESSION]);
    assert!(ok, "{err}");
    check("fixture-0001.show.txt", &out);
}

#[test]
fn the_sessions_table_is_golden() {
    let (out, err, ok) = kagviz(&["sessions"]);
    assert!(ok, "{err}");
    assert!(err.contains("1 session(s) under"), "{err}");
    check("fixture-0001.sessions.txt", &out);
}

/// The seam: a report rendered from the serialized facts — through a file and
/// through stdin — is the report rendered from the transcript.
#[test]
fn a_report_from_the_facts_is_the_report_from_the_transcript() {
    let (facts, _, ok) = kagviz(&["show", SESSION, "--json"]);
    assert!(ok);
    let (direct, _, ok) = kagviz(&["render", SESSION]);
    assert!(ok);

    let scratch = manifest().join("target").join("golden-tests");
    fs::create_dir_all(&scratch).unwrap();
    let path = scratch.join(format!("{}-{SESSION}.facts.json", std::process::id()));
    fs::write(&path, &facts).unwrap();
    let (from_file, err, ok) = kagviz(&["render", "--from", path.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert_eq!(
        from_file, direct,
        "render --from <file> drifted from render <id>"
    );
    let _ = fs::remove_file(&path);

    let (from_stdin, err, ok) = kagviz_at(&root(), &["render", "--from", "-"], Some(&facts));
    assert!(ok, "{err}");
    assert_eq!(
        from_stdin, direct,
        "render --from - drifted from render <id>"
    );
}

/// `derive` over the fixture as if it were a mirrored host: the facts and
/// events it writes are the bytes `show` prints, and the index links them.
#[test]
fn derive_writes_the_same_bytes_show_prints_and_the_index_links_them() {
    let live = manifest()
        .join("target")
        .join("golden-tests")
        .join(format!("live-{}", std::process::id()));
    let _ = fs::remove_dir_all(&live);
    let projects = live.join("kai").join("projects");
    copy_tree(&root(), &projects);

    let (out, err, ok) = kagviz_at(&root(), &["derive", "--live", live.to_str().unwrap()], None);
    assert!(ok, "{err}");
    assert!(out.contains("kai"), "{out}");

    let derived = live.join("derived");
    let (facts, _, _) = kagviz(&["show", SESSION, "--json"]);
    let (events, _, _) = kagviz(&["show", SESSION, "--events"]);
    assert_eq!(
        fs::read_to_string(
            derived
                .join("facts")
                .join("kai")
                .join(format!("{SESSION}.json"))
        )
        .unwrap(),
        facts
    );
    assert_eq!(
        fs::read_to_string(
            derived
                .join("events")
                .join("kai")
                .join(format!("{SESSION}.json"))
        )
        .unwrap(),
        events
    );
    let sessions_json = fs::read_to_string(derived.join("sessions.json")).unwrap();
    let sessions: serde_json::Value = serde_json::from_str(&sessions_json).unwrap();
    let row = &sessions["sessions"][0];
    assert_eq!(row["host"], "kai");
    assert_eq!(row["session_id"], SESSION);
    assert_eq!(row["events"], format!("events/kai/{SESSION}.json"));
    assert!(derived.join(row["report"].as_str().unwrap()).is_file());
    assert!(
        fs::read_to_string(derived.join("index.html"))
            .unwrap()
            .contains("events</a>")
    );
    check("fixture-0001.sessions.json", &stable_stamp(&sessions_json));
    let _ = fs::remove_dir_all(&live);
}

/// The one field of `sessions.json` that is not a function of the fixture:
/// `kagviz` is `<version> (<commit>)`, so it moves on every commit and would
/// make the golden churn for reasons no reader cares about. Replaced with a
/// fixed placeholder — the *shape* is what this golden holds, and it is the
/// bytes the front-end's conformance test decodes.
fn stable_stamp(json: &str) -> String {
    let version = format!("{} ({})", env!("CARGO_PKG_VERSION"), env!("KAGVIZ_COMMIT"));
    assert!(
        json.contains(&version),
        "sessions.json no longer stamps `kagviz` as `{version}` — the placeholder is now a lie"
    );
    json.replace(&version, "<kagviz>")
}

/// The two documents are one pass: the events are what the facts counted.
/// Over the fixture, through the binary — the same invariants the unit
/// tests hold on synthetic records, on a session with every shape at once.
#[test]
fn the_events_add_up_to_the_facts() {
    let (facts_json, _, ok) = kagviz(&["show", SESSION, "--json"]);
    assert!(ok);
    let (events_json, _, ok) = kagviz(&["show", SESSION, "--events"]);
    assert!(ok);
    let facts: serde_json::Value = serde_json::from_str(&facts_json).unwrap();
    let events: serde_json::Value = serde_json::from_str(&events_json).unwrap();

    let sum = |m: &serde_json::Value| {
        m.as_object()
            .unwrap()
            .values()
            .map(|v| v.as_u64().unwrap())
            .sum::<u64>()
    };
    let list = events["events"].as_array().unwrap();
    let tools = list.iter().filter(|e| e["kind"] == "tool");
    assert_eq!(tools.clone().count() as u64, sum(&facts["tool_calls"]));
    assert_eq!(
        list.iter().filter(|e| e["kind"] == "turn").count() as u64,
        facts["assistant_turns"].as_u64().unwrap()
    );
    let unknown = facts["tool_failures"]["<unknown>"].as_u64().unwrap_or(0);
    assert_eq!(
        tools.clone().filter(|e| e["failed"] == true).count() as u64,
        sum(&facts["tool_failures"]) - unknown
    );
    assert_eq!(
        tools.clone().filter(|e| e["opaque"] == true).count() as u64,
        facts["changes"]["opaque_edits"].as_u64().unwrap()
    );
    assert_eq!(
        tools
            .clone()
            .filter_map(|e| e["lines_added"].as_u64())
            .sum::<u64>(),
        facts["changes"]["lines_added"].as_u64().unwrap()
    );
    // Per phase, and per spawn.
    //
    // Failures carry the same `<unknown>` carve-out per phase as the session
    // does: a failure whose call is not in the file still lands in the phase
    // its result was recorded in — a phase must not report an unknown as a
    // zero either — and the events still have no call to hang it on. The
    // fixture has exactly one, in its last phase.
    let mut unplaced = 0u64;
    for (i, phase) in facts["phases"].as_array().unwrap().iter().enumerate() {
        let of_phase = tools.clone().filter(|e| e["phase"] == i as u64);
        assert_eq!(
            of_phase.clone().count() as u64,
            phase["tool_calls"].as_u64().unwrap(),
            "phase {i} tool_calls"
        );
        let placed = of_phase.filter(|e| e["failed"] == true).count() as u64;
        let counted = phase["tool_failures"].as_u64().unwrap();
        assert!(
            placed <= counted,
            "phase {i} placed more failures than it counted"
        );
        unplaced += counted - placed;
    }
    assert_eq!(
        unplaced, unknown,
        "every unplaced phase failure is an <unknown>"
    );
    let spawns = facts["delegation"]["spawns"].as_array().unwrap();
    let spawn_events = events["spawns"].as_array().unwrap();
    assert_eq!(spawns.len(), spawn_events.len());
    for (s, e) in spawns.iter().zip(spawn_events) {
        assert_eq!(s["agent_id"], e["agent_id"]);
        let n = e["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|x| x["kind"] == "tool")
            .count() as u64;
        assert_eq!(n, sum(&s["tool_calls"]));
    }
    assert!(!events_json.contains("null"), "absent, never null");
    assert!(!facts_json.contains("null"), "absent, never null");
}

#[test]
fn an_unknown_session_is_an_error_not_an_empty_report() {
    let (out, err, ok) = kagviz(&["show", "no-such-session"]);
    assert!(!ok);
    assert!(out.is_empty());
    assert!(err.contains("no session no-such-session"), "{err}");
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}
