# Fixtures

`root/` is a transcript root shaped exactly like `~/.claude/projects`:

```
root/-home-ken-src-example/
    fixture-0001.jsonl                    # the session
    fixture-0001/subagents/agent-ex01.jsonl   # one spawned agent's sidecar
```

**Hand-written, every line.** Nothing here was copied off a host or off the
pinned corpus under `/ai-data` — that volume holds raw session content and
stays there. The session is synthetic: a made-up project, made-up paths, no
credentials, no real output. What it borrows from the corpus is the *shapes*,
one of each trap `docs/transcript-format.md` names:

- a bare-string prompt; a prompt with an `<ide_opened_file>` sibling block;
  a pasted image with text; a slash command's `<command-*>` scaffold followed
  by its `isMeta` skill body; a `[Request interrupted …]` placeholder; a
  resume nudge (`isMeta`) that opens a span with nobody asking
- `Edit` with a `structuredPatch`; `Write` as a `create`; two `Bash` calls,
  one failed, one interrupted with no result at all; a `Bash` whose result
  was offloaded to `tool-results/` and left a `<persisted-output>` placeholder
- `mcp__kaed-*__edit` twice: once returning its unified diff inside a JSON
  string, once `{"applied":true,"files":[…]}` with no diff at all
- `AskUserQuestion` with its answer joined through `toolUseResult.answers`
- an `Agent` call joined to a sidecar (`agentId` on the result) and one that
  is not (`unjoined_spawns`); a `Skill` call; an org-tool (`mcp__korg__*`) call
- a `tool_result` for a call that is not in the file (`<unknown>`)
- `"output_tokens_details": null` (trap 4); an unknown record type
  (`file-history-snapshot`); a CLI upgrade mid-session (`version` changes)
- an idle gap over two hours, so there are two spans; five phases of four kinds

The goldens in `../golden/` are what kagviz produces from it — facts, events,
the report and the `sessions` table — and `tests/golden.rs` compares the built
binary's output byte for byte. When a change moves them *on purpose*:

```sh
KAGVIZ_UPDATE_GOLDEN=1 cargo test --test golden
git diff tests/golden/    # read it: every changed line is a changed number or a changed page
```

A golden diff is the review surface. Wording changes in the report show up as
wording changes; a moved number shows up as a moved number; the point is that
both show up.
