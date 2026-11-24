# Super Browser

Super Browser (Super for short) is a web browser written in rust. it has its own html, css, dom, layout and paint engine, uses v8 (through rusty_v8) for javascript, renders with vello on wgpu, and draws its own browser ui. license is mit or apache-2.0 at your option.

this file is the entry point for every agent and every human. read it fully before doing anything.

## naming

- the product is "Super Browser", or "Super" for short. always capitalized like that in user-facing text: window titles, the about page, release notes, the readme, license headers.
- code identifiers, the binary (`super`, `super-headless`), crate names (`sp_*`), the repo and `super://` urls stay lowercase.
- the copyright holder is "the Super Browser contributors".

## rules

- the repo is the only context. everything needed to pick up any task cold lives in this repo. if a fact isn't in the repo, no agent may rely on it.
- no memory systems, no scratch notes outside the repo, no CLAUDE.md. tool-specific files are thin pointers back to AGENTS.md and agents/.
- .claude/ holds claude code wiring only (subagent definitions that point at agents/roles/). every rule lives in agents/ and docs/ so any tool follows the same rules.
- nothing operator-specific goes in the repo: no local paths, usernames, hostnames, keys, editor config, or personal tooling. it's shared with everyone or it doesn't go in.
- web behavior comes from the specs (whatwg, w3c, ietf, ecma). architecture comes from docs/design/. what's built is whatever the code says.
- agents don't write prose docs about what they built. no architecture writeups, no "how it works" files, no readme sections. docs/design/ changes only through a design-change issue that the owner approves.
- comments cite the spec algorithm and step they implement and explain why when the code alone can't. they don't narrate what the code does.
- every change goes through a pr and the process in agents/verification.md. nobody pushes to main.

## where things are

```
AGENTS.md                 this file
KICKOFF.md                the prompt that starts an orchestrator
agents/                   agent behavior: core rules, message formats, verification, roles
docs/design/              architecture (owner-approved changes only)
docs/milestones.md        milestone checklists, the orchestrator checks items off
crates/                   the engine and browser, one crate per subsystem (see docs/design/overview.md)
tests/wpt/                web-platform-tests, upstream submodule, never edited
tests/wpt-meta/           our expected results per wpt test
tests/html5lib-tests/     upstream submodule, never edited
tests/sites.list          the site ladder (docs/design/testing.md)
xtask/                    build and test tooling
.claude/agents/           claude code subagent pointers
```

## commands

```
cargo xtask check                    fmt, clippy -D warnings, unit tests, cargo-deny
cargo xtask wpt <path>...            run wpt tests, fail on any result that differs from tests/wpt-meta
cargo xtask wpt --update <path>...   rewrite expectations from the current run
cargo xtask html5lib                 tokenizer and tree construction suites
cargo xtask run [url]                launch the browser
cargo xtask headless <url> --png <out>   render one page offscreen
cargo xtask ladder                   screenshot every url in tests/sites.list
```

these commands are the contract. if one doesn't exist yet, building it is part of milestone m0.
