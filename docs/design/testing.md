# testing

## wpt

- web-platform-tests is a git submodule at tests/wpt, pinned. bumping the pin is its own pr with all resulting expectation changes.
- `cargo xtask wpt` starts wptserve (the server inside wpt, needs python 3), runs tests in super-headless, and compares results against tests/wpt-meta/.
- super-headless runs the same engine and compositor as the browser, rendering through wgpu to an offscreen texture. ci uses mesa's lavapipe as the vulkan adapter on linux.
- supported test types: testharness.js, reftests (with the fuzzy metadata wpt defines), crashtests, and print reftests after printing exists.
- expectations: one toml file per test file under tests/wpt-meta/ mirroring wpt paths, holding the expected status of the test and each subtest. tests with no meta file are expected to pass. any result that differs from the meta file counts as unexpected and fails ci, including unexpected passes, so stale expectations get caught in both directions.
- tests/wpt-meta/smoke.list holds a few hundred fast tests across all areas, run on every pr.
- the full suite runs nightly, sharded across runners. the nightly job posts pass counts per top-level directory to a tracking issue.

## other suites

- html5lib-tests (submodule) for the tokenizer and tree builder through `cargo xtask html5lib`.
- css-parsing-tests (submodule) for sp_css syntax.
- unit tests in each crate for anything below spec-visible behavior.

## site ladder

tests/sites.list is an ordered list of real urls, easy to hard. `cargo xtask ladder` renders each one headless and saves screenshots. nightly ci also screenshots each url with headless chromium and records a pixel difference score per site. the ladder never gates merges. it shows how far real pages get, and the milestones name ladder rungs as goals.

starting list (the orchestrator may append, the owner may reorder):

1. example.com
2. a wikipedia article
3. news.ycombinator.com
4. a github repository page
5. developer.mozilla.org reference page
6. a google search results page
7. reddit front page
8. a youtube watch page (layout only, no playback)
9. discord's web login page
10. figma's landing page

## fuzzing

cargo-fuzz targets for the html tokenizer, the css parser, the url parser, image decoders, and every ipc message a renderer can send. each target runs for a few minutes on every pr that touches its crate and continuously on a nightly job from m5.
