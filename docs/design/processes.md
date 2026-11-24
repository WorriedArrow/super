# processes, ipc, sandbox

## process model

- **browser** (one): ui, compositor, navigation, session history, permissions, profile storage, and every policy decision that has to hold up against a compromised renderer.
- **network** (one per profile): all network i/o, the cookie jar, the http cache, dns. it trusts nothing from renderers that the browser hasn't vouched for.
- **renderer** (one per site instance): html, css, dom, js, layout, paint. a site is scheme plus registrable domain (public suffix list). top-level navigations to a different site get a different renderer from m0 on.

cross-site iframes render out of process from m5. until then they render inside the parent's renderer, and the browser marks that renderer as mixed-site so later code paths can refuse to trust it.

the compositor is a thread in the browser process and takes nothing but messages. a separate gpu process comes later, and with that interface it's a move rather than a rewrite.

all processes are the same `super` binary, re-executed with `--type`. `--single-process` runs everything as threads for debugging and tests. production paths never assume it.

## ipc

- control messages: unix domain sockets on linux and macos, named pipes on windows. postcard encoding over serde. every message type lives in sp_ipc.
- bulk data (response bodies, decoded images, display lists): shared memory regions plus a control message carrying the handle.
- every message a renderer sends gets validated in the receiver before use. a renderer that sends something invalid (unknown frame id, origin it doesn't own, out-of-range offsets) gets killed. message decoding is a fuzz target.
- all processes come from one build, so no protocol versioning. a handshake checks the build id and kills mismatches.

## sandbox

renderer code is written as sandboxed from day 0: no fs or net access (enforced by clippy config, see agents/core.md). enforcement at the os level lands in m5:

- linux: user and pid and network namespaces, seccomp-bpf allowlist, no filesystem besides what's passed as fds.
- macos: sandbox_init with a deny-default profile.
- windows: restricted token, appcontainer, job object, win32k lockdown where wgpu allows it (renderers don't touch the gpu, so it should).

the network process gets a lighter sandbox: network allowed, filesystem limited to the profile's cache and cookie directories.

## crashes

a renderer crash shows a crash page in every tab it hosted. the browser process never trusts a renderer to exit cleanly and reaps it with a timeout. crash dumps go to the profile directory only, nothing gets uploaded.
