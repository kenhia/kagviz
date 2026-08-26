---
name: deploy-kagviz
description: Deploy kagviz on kai from committed main — rebuild the release binary the nightly timer runs, publish the web app to derived/app/, re-derive the served tree with that binary, and verify the stamp and the bytes. Use when asked to deploy/redeploy/ship kagviz, or when sprint-ship reaches Phase 7. Deploys committed code only.
---

# Deploy kagviz

**The deploy artifact is `target/release/kagviz` in this checkout.** Not a
container, not a package-store version — the systemd user unit sets
`WorkingDirectory` to this repo and `collect/collect.sh` runs
`$REPO/target/release/kagviz` directly. Whatever binary is sitting in that
directory at 04:00 is the extractor that derives the fleet's facts.

That is the whole reason this skill exists. Nothing else in the workflow
rebuilds it, so without a deploy step a ship lands on `main` and the nightly
collector keeps running the previous sprint's code. Sprint 011 found the live
tree stamped `0.1.0 (19a75d4)` — a sprint-009 **branch** commit that squash-merge
had collapsed, so it named no commit reachable from `main`. Three sprints.

## What is deployed, and where

Everything is on **kai**, which owns `/ai-data` (local NVMe). There is nowhere
else to run this.

| Artifact | Path | Written by |
|---|---|---|
| the extractor | `target/release/kagviz` in this checkout | `cargo build --release` |
| the served tree | `/ai-data/kagviz-data/live/derived/` | `kagviz derive` |
| the app bundle | `…/derived/app/` | `just web-deploy` |

`…/live/<host>/projects/` — the verbatim mirrors — are **never** touched by a
deploy, and never pruned. That is what makes `derived/` disposable: it rebuilds
from the mirrors in seconds.

## Deploy from clean, committed `main` — never a branch

A branch build stamps every facts file, every events document and every
`state.json` row with a commit that disappears at squash-merge. The tree then
claims to have been derived by something you cannot check out, which is a
rollback target nobody can reproduce — and `derive`'s "re-derive when the
version differs" rule silently churns the whole fleet the next time anyone
builds from `main`.

This is why sprint-ship deploys in Phase 7, after the merge and after `main` is
pulled. Preserve that ordering.

**Refuse to deploy** if `git status --short` is non-empty, if `HEAD` is not
`main`, or if `main` is behind `origin/main`.

## Procedure

Order matters at one point, called out below.

1. **Preflight.** On kai; tree clean; on `main`; up to date with `origin/main`.
   Record `git rev-parse --short=7 HEAD` — every check below is against it.

2. **Snapshot the derived tree**, so step 6 can prove what moved:

   ```sh
   cd /ai-data/kagviz-data/live/derived
   find facts events reports -type f | sort | xargs sha256sum > /tmp/kagviz-before.sha
   ```

3. **Deploy the app first.** `just web-deploy` builds `web/` and stages the
   bundle into `derived/app/`, renaming into place.

   **This must precede step 4.** `derive` regenerates `index.html` at the end of
   its run, and the browse page links the app *only when `app/index.html` is
   already on disk* (`derive::APP_ENTRY`). Deploy the app after the derive and
   the page ships without its link until someone runs `kagviz index` by hand.

   Skip only if `web/` is unchanged since the last deploy **and** `derived/app/`
   already exists — and say that you skipped it.

4. **Rebuild and re-derive.** `just collect-derive` — the recipe depends on
   `build-release`, so this both rebuilds the binary and derives with it. It
   re-derives every session because the version stamp changed; that is the
   design, and it is seconds, not minutes.

   `just collect` would also sync the fleet first. Do **not** use it here: a
   deploy should not be the thing that decides whether cleo was reachable. The
   04:00 timer owns syncing.

5. **Verify the stamp.** `derive` prints the binary's own stamp on its last
   line and writes it into `META.json`, so those two agreeing with `HEAD` is
   the whole check — no need to read the binary:

   ```sh
   git rev-parse --short=7 HEAD
   python3 -c "import json;print(json.load(open('/ai-data/kagviz-data/live/derived/META.json'))['kagviz'])"
   ```

   `META.json` must read `<crate version> (<HEAD>)`. If it names some other
   commit, the binary that derived the tree is not the one you just built —
   stop, because that is the condition this skill exists to catch.

6. **Verify the bytes — this is the real check.** Re-run the `sha256sum` sweep
   from step 2 and diff it.

   **A sprint that did not change the extractor must move zero derived bytes.**
   The stamp changes; no value does. Anything else is a change to the facts, and
   `docs/facts-contract.md` requires it to be named and measured rather than
   discovered later.

   - No diff → say so with the file count. That is the deploy's proof.
   - Diff, and the sprint changed the extractor on purpose → report **how many
     documents moved and what moved in them**, and check the sprint record
     actually carries that measurement.
   - Diff, and the sprint did *not* touch the extractor → **stop and report.**
     Something changed the facts that nobody meant to.

7. **Verify it is served.** Over copyparty, not the filesystem:

   ```sh
   curl -sk -o /dev/null -w '%{http_code}\n' https://kai.encke-wahoo.ts.net:8027/kagviz/index.html
   curl -sk -o /dev/null -w '%{http_code}\n' https://kai.encke-wahoo.ts.net:8027/kagviz/app/index.html
   curl -sk https://kai.encke-wahoo.ts.net:8027/kagviz/index.html | grep -c 'href="app/index.html"'
   ```

   Two 200s and one match. The third is the step-3 ordering trap, caught.

8. **Smoke-test what the sprint actually changed** against the deployed
   instance — not just that it is up. A changed panel, a new field, a moved
   number: look at it. If the sprint touched the app, open a session page.

## What this skill can and cannot assert

Say it plainly in the report.

**Can assert**: the binary the timer will run at 04:00 is built from a commit on
`main`; the served tree was derived by that exact binary; exactly which derived
documents moved and which did not; that copyparty serves both pages and the
browse page links the app.

**Cannot assert** that the *next* nightly run succeeds — it depends on hosts
being reachable, which is a normal nightly variable and not a deploy property.
Nor that the mirrors are current: a deploy deliberately does not sync.

`just collect-status` is the check for the run itself, the morning after.

## Rollback

Cheap, because nothing here is the source of truth. The mirrors are untouched
and never pruned, so `derived/` is fully regenerable.

```sh
git checkout <previous-commit>
just web-deploy          # the app bundle at that commit
just collect-derive      # rebuild the binary, re-derive with it
```

`--force` is only needed if the version stamp did not change and you still want
everything rebuilt. Then `git checkout main` when done — leaving the checkout on
a detached commit leaves the *timer* on it too, which is the same staleness this
skill exists to prevent, wearing a different hat.

## Failure

A failed deploy does not roll back the merge — the code is good, the rollout is
not. Leave `main` alone, follow the rollback above, and report what shipped to
git versus what reached the served tree.

The one genuinely bad state is a **half-deployed tree**: a new binary with an
interrupted derive. It is also self-healing — `derive` skips sessions whose
digest *and* version already match, so re-running finishes the job. Re-run it
rather than reaching for `--force`.
