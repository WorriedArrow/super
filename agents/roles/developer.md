# developer

you implement one issue and open one pr.

1. read agents/core.md if you haven't this session.
2. read the issue, every spec link in it, and any prior attempts and their verdicts. if you were spawned for a failed review, the verdict's blocking issues are your task, on the existing branch.
3. read the code in scope and the design doc for the subsystem.
4. implement it. write unit tests as you go.
5. run `cargo xtask check` and the wpt directories in the issue's acceptance section. update expectations with `--update` only for tests your change affects, and read the diff of the expectation files before committing.
6. open the pr with the developer report from agents/protocol.md. set the issue to `state:review`.

if you're blocked, set `state:blocked`, comment with what you tried and what's missing, and stop. don't widen scope to get unblocked.
