# network

everything here runs in the network process on tokio, except where noted.

## fetch

- the fetch spec is implemented in sp_net, not borrowed from a library: request and response objects, main fetch, http fetch, http-network-or-cache fetch, cors and preflight, redirects, referrer policy, mixed content blocking, orb (opaque response blocking), and fetch metadata headers.
- the browser process tells the network process which origin and site each renderer represents. the network process takes initiator origins from that table, never from the renderer's message.
- csp checks for subresources happen in the renderer, which has the policy. navigation checks happen in the browser.

## protocols

- http/1.1 and http/2 through hyper and h2. http/3 through quinn and h3 in m4, with alt-svc discovery.
- tls through rustls with rustls-platform-verifier, so certificate trust follows the os store.
- dns through hickory-resolver using system config. dns over https comes later as a setting.
- websockets in m4. data:, blob: and about: urls get handled without network i/o. file: urls go through the browser process with an explicit user navigation.

## state

- cookies per rfc 6265bis: samesite, prefixes, partitioned cookies (chips) in m5. the public suffix list comes vendored at a pinned version.
- http cache per rfc 9111, stored on disk in the profile, keyed by top-level site and frame site (cache partitioning) from day 0.
- hsts with chromium's preload list, vendored and pinned.

## storage (sp_storage, browser process)

- profile directory layout, history, bookmarks, session restore.
- localStorage and sessionStorage in m4, keyed by storage key (top-level site plus origin).
- indexeddb in m4, backed by sqlite through rusqlite.
