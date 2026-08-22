# kagviz

Visualize how an agent session actually went.

A Claude Code session leaves a transcript on disk. kagviz reads it and reports
what happened: which tools ran and which failed, what changed on disk, where
the time went, and where the user was involved. The aim is **insight, not
debugging** — a picture of how the work got done, for the person who asked for
it.

The extraction is deterministic. Every number is a pure function of the
transcript bytes, so the same session yields the same report forever. Where a
model is used at all, it is only to *write the headline* over facts that were
already counted — never to produce the facts.

## Status

Early. The deterministic core reads transcripts and summarizes them; the static
HTML report is the next milestone. See `sprints/planning/roadmap.md`.

```console
$ kagviz sessions
SESSION                                PROJECT                 ACTIVE   TOOLS   FAIL
63a9b83b-9e7b-4f77-8ac2-df66ecd0407e   -home-ken-5090             38m      83      2

$ kagviz show 63a9b83b-9e7b-4f77-8ac2-df66ecd0407e
time      38m active / 2h30m wall (1h52m idle)
turns     226 assistant, 26 user prompts
tools     83 calls, 2 failed
            28  Bash  (2 failed)
            19  Read
             5  Edit
files     3 touched, +57/-3  [28 opaque shell call(s) unaccounted]
```

`kagviz show <id> --json` emits the whole facts document. That JSON is the
contract: renderers consume it, and it is the seam a future interactive
front-end plugs into.

## Reading the output honestly

Two numbers deserve care, and both are there because leaving them out would
quietly lie:

- **active vs wall** — a resumed session can span days and hold an hour of
  work. Gaps of 2 minutes or more are counted as idle, not effort.
- **opaque shell calls** — edits made through the shell leave no recoverable
  diff, so the line deltas are a floor, not a total.

`docs/transcript-format.md` documents the on-disk format, including the traps.
It is a field guide, not a spec: the format is undocumented and drifts between
CLI releases.

## Where it fits

- **harness-eval** scrapes session metrics ad hoc for its run logs; kagviz aims
  to be the thing that does that properly.
- **ai-findings** pairs field notes with infographics, and is the natural first
  consumer of a kagviz report.

## Development

Built on the [kprojects](https://github.com/kenhia/kprojects) minimal harness.

```sh
just          # list recipes
just check    # fmt + clippy + tests (the CI gate)
just fmt      # apply formatting
```

## License

MIT — see [LICENSE](LICENSE).
