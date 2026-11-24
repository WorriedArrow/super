# milestones

the orchestrator checks items off as they merge. pass-rate targets are for the listed wpt directories and the owner can change them. a milestone is done when every item is checked, every target is met, and the audit in agents/verification.md passes.

## m0: skeleton

- [ ] cargo workspace with every crate from docs/design/overview.md, empty where nothing exists yet
- [ ] LICENSE-MIT and LICENSE-APACHE copied verbatim from upstream
- [ ] xtask with every command in AGENTS.md, stubbed where the subsystem doesn't exist yet
- [ ] ci on linux, macos and windows running `cargo xtask check`, cargo-deny with the deps.md license list, clippy disallowed-methods for renderer crates
- [ ] wpt, html5lib-tests and css-parsing-tests submodules pinned
- [ ] `super --type` re-exec with browser, network and renderer processes, ipc handshake, `--single-process`
- [ ] window with the chrome from docs/design/ui.md: tabs, url bar, back, forward, reload, drawn with vello
- [ ] network process fetching http and https through hyper and rustls, streaming bodies to the renderer
- [ ] renderer showing the raw response text, painted through a display list and the compositor
- [ ] new renderer per site on top-level navigation
- [ ] super-headless plus the wpt runner: starts wptserve, runs tests, writes and compares expectations, even though nearly everything fails

## m1: static documents

- [ ] sp_url, target 100% of wpt url/
- [ ] encoding sniffing and decoding, target 95% of wpt encoding/ (non-js parts)
- [ ] html tokenizer and tree construction, 100% of html5lib-tests with scripting off
- [ ] dom core: tree, algorithms from the dom spec, no js yet
- [ ] css syntax (100% of css-parsing-tests), properties.toml codegen, selectors level 4 minus :has
- [ ] cascade, inheritance, computed values, user-agent stylesheet from the html spec
- [ ] block layout with margin collapsing, inline layout with line breaking and bidi, floats, lists
- [ ] text: system fonts with fallback, harfrust shaping, icu4x segmentation
- [ ] backgrounds, borders, border-radius, colors, images (png, jpeg, gif, webp)
- [ ] links, navigation, session history, scrolling in the compositor
- [ ] target 70% of wpt css/CSS2/normal-flow, css/CSS2/floats, css/CSS2/linebox
- [ ] ladder: rungs 1 to 3 readable with correct text flow

## m2: scripting

- [ ] research: dom and gc ownership (docs/design/js.md), decided through a design-change issue
- [ ] rusty_v8 integration, realms, the html event loop and microtask checkpoints
- [ ] sp_webidl codegen from vendored webref idl, every interface exposed
- [ ] dom bindings: nodes, elements, attributes, events and dispatch, collections, mutation observers
- [ ] parser script interleaving, document.write, html5lib-tests with scripting on
- [ ] timers, rAF, console, fetch(), xhr, url and urlsearchparams, textencoder and textdecoder
- [ ] es modules, import maps, dynamic import
- [ ] cssom: getComputedStyle, element.style, stylesheets api, getBoundingClientRect, offset and client geometry
- [ ] forms enough to submit a search (text input, button, form submission)
- [ ] target 80% of wpt dom/, 60% of wpt html/webappapis/
- [ ] ladder: rungs 1 to 5 render and their scripts run without uncaught errors from unimplemented core apis

## m3: modern layout and compositing

- [ ] flexbox, target 85% of wpt css/css-flexbox
- [ ] positioning complete (absolute, fixed, sticky), overflow and scroll containers, z-index and stacking
- [ ] transforms, opacity, filters, gradients, box shadows, clip-path basic shapes
- [ ] css transitions and animations, transform and opacity running in the compositor
- [ ] incremental style and layout, parallel styling with rayon
- [ ] speculative preload scanner
- [ ] web fonts (woff, woff2) through @font-face
- [ ] ladder: rungs 1 to 7

## m4: applications

- [ ] css grid, target 80% of wpt css/css-grid
- [ ] tables
- [ ] form controls: every input type, select, textarea, ime, validation, autofill off
- [ ] shadow dom, custom elements, :has(), @container, @layer complete
- [ ] inline svg rendered by the engine, svg as an image
- [ ] canvas 2d through vello
- [ ] web storage, indexeddb, dedicated workers, websockets, http/3
- [ ] ladder: all rungs

## m5: isolation and hardening

- [ ] out-of-process cross-site iframes
- [ ] os sandboxes on linux, macos and windows per docs/design/processes.md
- [ ] orb, partitioned cookies, site-keyed network state enforced in the network process
- [ ] continuous fuzzing of every target in docs/design/testing.md
- [ ] v8 startup snapshot

## m6: polish

- [ ] accessibility through accesskit for chrome and pages
- [ ] gpu process split out of the browser process
- [ ] webdriver bidi subset, enough to run wpt through upstream wptrunner as a cross-check
- [ ] performance pass driven by profiles of the ladder sites
