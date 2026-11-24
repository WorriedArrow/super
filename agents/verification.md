# verification

every pr goes through all of this before merge. no exceptions for small changes.

## 1. ci

- `cargo xtask check` on linux, macos and windows.
- `cargo xtask wpt` on every wpt directory mapped to the touched crates (mapping lives in xtask), plus tests/wpt-meta/smoke.list. any unexpected result fails ci.
- `cargo xtask html5lib` when sp_html changes.
- for changes to style, layout, paint, text or the compositor: `cargo xtask ladder`, with screenshots uploaded as ci artifacts next to the main-branch run for comparison. ladder results don't gate merges, they go to the reviewer.

## 2. review

the orchestrator spawns a fresh reviewer agent for each pr. the reviewer never wrote any of the code under review and gets only the issue, the pr diff, the developer report, and the repo.

the reviewer checks:

- each touched spec algorithm, step by step, against the code.
- the rules in agents/core.md, with special attention to test special-casing, faked success, panics reachable from web content, and i/o in renderer crates.
- the developer's claims, by rerunning the commands in the report.
- whether expectation changes match the code change. a large fail to pass jump from a small diff gets examined test by test.
- ladder screenshots for visual changes.

the reviewer writes a verdict per agents/protocol.md and never suggests fixes.

## 3. failed review

the orchestrator never sends a failed pr back to its author. it spawns a brand new developer agent with the issue, the branch, and the verdict, and that agent fixes the pr on the same branch. after three failed rounds on one issue, the orchestrator splits the issue or opens a kind:research issue about the blocker before trying again.

## 4. merge

the orchestrator squash-merges once the verdict passes and ci is green. the commit message is the issue title plus `closes #n`.

## 5. milestone audit

before checking off a milestone, the orchestrator spawns a fresh reviewer to audit ten randomly chosen merged prs from that milestone against this file. any finding becomes a kind:bug issue that blocks the milestone.
