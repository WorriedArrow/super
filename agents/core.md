# core behavior

every agent follows this file regardless of role.

## before writing code

- read the issue, the spec sections it links, and the code you're about to change. read the spec algorithm in full before implementing any step of it.
- check docs/design/ for the subsystem. if the issue conflicts with the design, stop and report it instead of picking one.

## implementing web behavior

- implement spec algorithms as written, step by step. name functions after the algorithm ("insert a node" becomes `insert_node`). put the spec url with anchor above the function and step numbers on the lines that implement them, e.g. `// step 7.2: if child is a doctype...`.
- when the spec and other browsers disagree, follow the spec unless wpt tests the browser behavior, then follow wpt. note the disagreement in the pr report.
- unimplemented features follow the spec's failure path (throw NotSupportedError, ignore the property, treat as unknown element) and log `unimplemented: <feature>` once per process. never fake success.
- never special-case a test: no branches on wpt file names, test urls, harness globals, or site hostnames. one instance gets the pr rejected and audited.
- anything a web page controls must never panic or abort a process. malformed input takes the spec's error-handling path. a renderer crash kills only its own tabs.

## code rules

- rust stable, edition 2024. `cargo xtask check` passes with zero warnings.
- `unsafe` only in sp_js, sp_ipc, sp_sandbox and sp_compositor. every unsafe block gets a `// safety:` comment saying what makes it sound.
- renderer-side crates (everything sp_renderer depends on) never touch the filesystem or network. clippy's disallowed-methods config blocks std::fs and std::net there. all i/o goes through ipc to the browser or network process.
- dependencies only per docs/design/deps.md. anything else needs a design-change issue.
- don't copy code from other engines (blink, webkit, gecko, servo, ladybird). reading them to understand an approach is fine, and researchers cite them with links.
- no quadratic work on dom size or style rule count in hot paths. no other optimization before a profile shows the need.

## tests

- every behavior change comes with tests: unit tests in the crate, plus the wpt expectation changes it causes.
- never edit tests/wpt or tests/html5lib-tests. they're upstream submodules.
- expectation changes ride in the same pr as the code that causes them. a test going pass to fail needs a stated reason in the pr report, and "unrelated" doesn't count.
- report exactly what you ran and what it printed. no "tests pass" without the command.

## scope and size

- one issue, one pr. keep prs under about 1500 changed lines, not counting generated code and expectation files. split anything bigger and say so in the report.
- stay inside the files and crates the issue scopes. if the fix needs something outside it, report that instead of widening the pr.
- after two failed attempts at the same problem, stop and report it as blocked with what you tried.

## communication

- you never talk to the owner. questions go to the orchestrator in your report, using agents/protocol.md.
- write plainly. lowercase, no marketing words, no filler. say what changed and what didn't work.
