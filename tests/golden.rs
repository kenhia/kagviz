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
    // `tool_calls` is an equality per phase. Failures are **not**, and this
    // test asserted that they were until sprint 012 measured it: the facts
    // count a failure on the record carrying the *result*, the event carries
    // `failed` on the *call*, so a call whose result came back after a phase
    // boundary is counted in one phase and drawn in the next — in either
    // direction. 17 phases of the 413-session corpus place more failures than
    // their phase counts; the fixture has none, which is the only reason the
    // old `placed <= counted` held (and it would have underflowed the running
    // total, not merely failed). What is true is the signed sum: across the
    // phases the shortfall is exactly the `<unknown>` count.
    let mut unplaced = 0i64;
    for (i, phase) in facts["phases"].as_array().unwrap().iter().enumerate() {
        let of_phase = tools.clone().filter(|e| e["phase"] == i as u64);
        assert_eq!(
            of_phase.clone().count() as u64,
            phase["tool_calls"].as_u64().unwrap(),
            "phase {i} tool_calls"
        );
        let placed = of_phase.filter(|e| e["failed"] == true).count() as i64;
        unplaced += phase["tool_failures"].as_i64().unwrap() - placed;
    }
    assert_eq!(
        unplaced, unknown as i64,
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

#[test]
fn the_calls_are_golden() {
    let (out, err, ok) = kagviz(&["show", SESSION, "--calls"]);
    assert!(ok, "{err}");
    check("fixture-0001.calls.json", &out);
}

/// The calls are the events' payload half, and the join is the contract.
///
/// One entry per `tool` event across both tiers, joined by `tool_use_id`,
/// and the two sizes the events carry are the lengths of the two things the
/// calls carry. Those hold by construction — both documents are filled from
/// the same block in the same iteration — so this test is here to catch that
/// construction being taken apart, which is exactly how it would break.
#[test]
fn every_tool_event_has_its_text_and_the_sizes_agree() {
    let (events_json, _, ok) = kagviz(&["show", SESSION, "--events"]);
    assert!(ok);
    let (calls_json, _, ok) = kagviz(&["show", SESSION, "--calls"]);
    assert!(ok);
    let events: serde_json::Value = serde_json::from_str(&events_json).unwrap();
    let calls: serde_json::Value = serde_json::from_str(&calls_json).unwrap();

    // Both tiers, flattened the way the calls document flattens them.
    let mut tools: Vec<&serde_json::Value> = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "tool")
        .collect();
    for spawn in events["spawns"].as_array().unwrap() {
        tools.extend(
            spawn["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["kind"] == "tool"),
        );
    }
    let list = calls["calls"].as_array().unwrap();
    assert_eq!(
        list.len(),
        tools.len(),
        "one calls entry per tool event, spawns included"
    );
    assert!(
        tools.iter().any(|e| e["phase"].is_null()),
        "the fixture must keep a spawn in this join, or it proves only the parent"
    );

    for event in &tools {
        let id = event["id"].as_str().expect("the fixture ids every call");
        let call = list
            .iter()
            .find(|c| c["id"] == event["id"])
            .unwrap_or_else(|| panic!("no calls entry for {id}"));
        assert_eq!(call["tool"], event["tool"], "{id}");

        // `input_bytes` *is* the canonical serialization's length, so the
        // input the calls document carries has to re-serialize to it.
        assert_eq!(
            call["input"].is_null(),
            event["input_bytes"].is_null(),
            "{id}: an input and its size are present together or not at all"
        );
        if !call["input"].is_null() {
            assert_eq!(
                serde_json::to_string(&call["input"]).unwrap().len() as u64,
                event["input_bytes"].as_u64().unwrap(),
                "{id}"
            );
        }

        // And the reading that matters most: absent is absent. An
        // interrupted call has no `result` key, not an empty one.
        assert_eq!(
            call["result"].is_null(),
            event["result_bytes"].is_null(),
            "{id}: a result and its size are present together or not at all"
        );
        if !call["result"].is_null() {
            assert_eq!(
                call["result"].as_str().unwrap().len() as u64,
                event["result_bytes"].as_u64().unwrap(),
                "{id}"
            );
        }
    }
}

/// The disclosure boundary, as a test.
///
/// `derive` writes no calls document; `derive --calls` writes it and the
/// index links it; `derive --drop-calls` takes it back off. This is the one
/// thing in the derived tree that is a decision rather than a consequence,
/// and the state machine behind it is easy to break in a way no other test
/// would notice — most of all the `--calls`-after-a-plain-run case, which
/// `state.json` alone would report as unchanged and skip.
#[test]
fn the_calls_document_is_written_only_when_asked_for() {
    let live = manifest()
        .join("target")
        .join("golden-tests")
        .join(format!("calls-{}", std::process::id()));
    let _ = fs::remove_dir_all(&live);
    copy_tree(&root(), &live.join("kai").join("projects"));
    let arg = live.to_str().unwrap();
    let derived = live.join("derived");
    let calls = derived
        .join("calls")
        .join("kai")
        .join(format!("{SESSION}.json"));
    let row_calls = || -> serde_json::Value {
        let raw = fs::read_to_string(derived.join("sessions.json")).unwrap();
        serde_json::from_str::<serde_json::Value>(&raw).unwrap()["sessions"][0]["calls"].clone()
    };

    let (_, err, ok) = kagviz_at(&root(), &["derive", "--live", arg], None);
    assert!(ok, "{err}");
    assert!(!calls.is_file(), "a plain derive must write no call text");
    assert!(
        row_calls().is_null(),
        "and the index must not link a document that is not there"
    );

    // The trap: same bytes, same kagviz, so `state.json` says unchanged. The
    // run must still notice that an output it was asked for is missing.
    let (_, err, ok) = kagviz_at(&root(), &["derive", "--live", arg, "--calls"], None);
    assert!(ok, "{err}");
    let (expected, _, _) = kagviz(&["show", SESSION, "--calls"]);
    assert_eq!(
        fs::read_to_string(&calls).unwrap(),
        expected,
        "derive --calls writes the bytes show --calls prints"
    );
    assert_eq!(row_calls(), format!("calls/kai/{SESSION}.json"));

    // A later plain run leaves it alone — `derived/` is regenerable, and a
    // run that was not asked about call text does not get to decide about it.
    let (_, err, ok) = kagviz_at(&root(), &["derive", "--live", arg], None);
    assert!(ok, "{err}");
    assert!(calls.is_file(), "a plain derive must not silently drop it");
    assert_eq!(row_calls(), format!("calls/kai/{SESSION}.json"));

    // Asking is the only way in, and asking is the only way back out.
    let (_, err, ok) = kagviz_at(&root(), &["derive", "--live", arg, "--drop-calls"], None);
    assert!(ok, "{err}");
    assert!(!calls.is_file(), "--drop-calls removes it");
    assert!(row_calls().is_null(), "and the index un-links it");
    let _ = fs::remove_dir_all(&live);
}
