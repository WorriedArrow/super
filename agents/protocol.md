# protocol

all coordination happens through github issues and prs on this repo. agents don't keep state anywhere else.

## labels

- state: `state:ready`, `state:in-progress`, `state:review`, `state:failed-review`, `state:blocked`, `state:done`
- kind: `kind:feature`, `kind:bug`, `kind:research`, `kind:design-change`, `kind:triage`
- milestone: `m0` through `m6`
- crate: `crate:<name>` for every crate the issue may touch. the orchestrator uses these to avoid parallel conflicts.

## task brief (issue body, written by the orchestrator or triage)

```
## goal
one or two sentences.

## spec
links with anchors to every algorithm involved.

## scope
crates and files that may change. anything else is out.

## acceptance
- wpt: paths that must have no unexpected results, plus tests expected to newly pass
- unit tests that must exist
- headless renders to check, if visual

## out of scope
things that look related but belong to other issues.

## prior attempts
links to failed prs and their review verdicts, if any.
```

## developer report (pr body)

```
closes #<issue>

## changed
what changed and why, per crate. short.

## ran
every command run and its result. wpt delta per directory as passing/total, before and after.

## expectations
fail to pass: count, plus the list if under 30.
pass to fail: every test, each with its reason.

## spec disagreements
places where wpt or other browsers differ from the spec, and which side the code follows.

## questions
anything the orchestrator needs to decide. empty if none.
```

## review verdict (pr review, written by a reviewer)

```
verdict: pass | fail

## issues
- [blocking] path/to/file.rs:123: what's wrong, which spec step or rule it breaks
- [minor] ...

## verified
commands the reviewer reran and what they printed.
```

reviewers never propose fixes or code. one blocking issue makes the verdict fail. minor issues alone pass.

## research report (comment on a kind:research issue)

```
## question
## answer
the recommendation, in one paragraph.
## evidence
spec links, wpt coverage, how other engines handle it (links to their source), measurements if any.
## options rejected
each with the reason.
## unknowns
what the evidence doesn't settle.
```

## triage report (issues filed by triage)

one issue per root cause, never per test. title names the cause ("margin collapsing ignores clearance"), body uses the task brief format with every affected test listed under acceptance.
