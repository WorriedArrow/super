# dependencies

the rule: anything a web spec defines gets built in super. infrastructure underneath the web platform comes from crates.

## built in super

html parsing, dom, url parsing, css syntax and every css module, selectors, cascade, layout, painting order, display lists, the compositor, webidl codegen and bindings, the event loop, fetch, cookies, http caching, hsts, csp, the browser ui widget set.

## allowed crates

| area | crates |
|---|---|
| js | v8 (rusty_v8) |
| gpu and 2d | wgpu, vello |
| windowing and input | winit, raw-window-handle, accesskit, accesskit_winit |
| fonts and text | fontique, skrifa, read-fonts, harfrust, icu (icu4x components), unicode-bidi, encoding_rs |
| images | png, zune-jpeg, image-webp, gif |
| network | tokio, hyper, http, bytes, http-body-util, hyper-util, tokio-rustls, futures-util, h2, quinn, h3, rustls, rustls-platform-verifier, hickory-resolver, brotli, flate2, zstd |
| storage | rusqlite (bundled sqlite) |
| ipc and serialization | serde, postcard, memmap2 |
| sandboxing | seccompiler, windows (the microsoft crate), nix |
| general | rayon, crossbeam, parking_lot, smallvec, bitflags, thiserror, tracing, tracing-subscriber, arrayvec, hashbrown, bumpalo |
| build and tooling | xshell, clap, toml, anyhow (xtask only), serde_json (xtask and test harnesses only, never linked into a shipped binary), cargo-fuzz targets |

## rules for any crate

- license compatible with mit or apache-2.0 distribution: mit, apache-2.0, bsd, isc, zlib, unicode, mpl-2.0. no gpl, lgpl or agpl. cargo-deny enforces it.
- maintained: a release or commit within the last year, or small and finished.
- one version per crate in the workspace where cargo allows it. cargo-deny warns on duplicates.
- adding anything not in the table above goes through a design-change issue, and the owner approves it.
