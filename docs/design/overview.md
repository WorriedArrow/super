# overview

super is a full desktop browser: tabs, url bar, history, downloads, multiple processes, and an engine written from scratch in rust apart from the pieces listed in deps.md.

## targets

- linux (x11 and wayland), macos, windows. linux is the primary ci platform and gets full wpt runs. macos and windows build and run the smoke set on every pr.
- x86-64 and arm64.

## not in v0

webgl, webgpu, webrtc, audio and video playback, extensions, sync, printing, a pdf viewer, eme/drm, vertical writing modes, devtools. pages that use these get the spec's "not supported" path. each one comes back as its own milestone after m6.

## crates

| crate | process | does |
|---|---|---|
| sp | all | package for the `super` and `super-headless` binaries. `--type=browser\|network\|renderer` picks the role, browser is the default |
| sp_browser | browser | tabs, navigation, session history, site isolation bookkeeping, permissions, profile |
| sp_ui | browser | browser chrome: window, tabs, url bar, menus, prompts, drawn with vello |
| sp_compositor | browser | layer tree, scrolling, compositor animations, vello scene, wgpu present |
| sp_ipc | all | channels, shared memory, every message type |
| sp_sandbox | all | per-platform process sandboxing |
| sp_net | network | fetch, http/1.1/2/3, tls, dns, cookies, http cache, hsts |
| sp_storage | browser | profile storage: history, bookmarks, web storage, indexeddb |
| sp_url | any | whatwg url parsing and serialization |
| sp_html | renderer | tokenizer and tree construction |
| sp_dom | renderer | node tree, dom algorithms, events, ranges, mutation observers |
| sp_css | renderer | css syntax, property definitions, values, selectors, media queries |
| sp_style | renderer | cascade, computed values, inheritance, invalidation |
| sp_text | renderer | font matching and fallback, shaping, line breaking, bidi |
| sp_layout | renderer | box tree, formatting contexts, fragment tree |
| sp_paint | renderer | stacking contexts and painting order, display list |
| sp_image | renderer | image decoding |
| sp_webidl | build | parses webidl, generates bindings |
| sp_js | renderer | v8 integration, realms, event loop, module loading |
| sp_bindings | renderer | generated bindings plus the hand-written parts of each interface |
| sp_renderer | renderer | renderer process main, ties the renderer crates together |
| xtask | dev | build, test, wpt, ladder tooling |

## shape of a page load

1. the url bar hands a url to sp_browser, which picks or spawns a renderer for the target site and asks the network process to start the navigation fetch.
2. response headers come back to the browser, which runs navigation checks (csp frame-ancestors, x-frame-options, coop, download vs render) and commits the navigation to the renderer.
3. the body streams from the network process straight to the renderer over a data pipe.
4. the renderer parses, runs scripts, styles, lays out and paints into a display list, then sends the display list to the compositor through shared memory.
5. the compositor builds a vello scene from the display lists of every visible frame and the chrome, renders it with wgpu and presents.
