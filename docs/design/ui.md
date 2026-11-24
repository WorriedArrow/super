# browser ui

## chrome

- drawn by sp_ui with vello in the browser process, using a small in-house widget set: tab strip, url bar, navigation buttons, menus, context menus, find bar, permission prompts, download shelf, dialogs (alert, confirm, prompt, beforeunload, http auth).
- the chrome doesn't depend on the engine rendering correctly, so it works from m0 even while the engine renders garbage.
- windowing and input through winit. ime events go to the focused renderer over ipc. accessibility through accesskit lands in m6 for both the chrome and the page tree.

## internal pages

- `super://newtab`, `super://settings`, `super://history`, `super://downloads`, `super://about` and the crash and error pages are html, css and js bundled into the binary and rendered by the engine, so they double as tests.
- they run in their own renderer with a privileged ipc channel that only this renderer gets. web content can never navigate to them or frame them.

## url bar

- url fixup per the whatwg url spec plus browser heuristics (bare hostnames, missing schemes, spaces mean search).
- search engine configurable in settings, with no default tracking params.
- suggestions from history and bookmarks.

## v0 features

tabs with drag to reorder and tear off into new windows, back and forward with session history, reload and stop, zoom, find in page, downloads, private windows (separate in-memory profile), session restore, bookmarks, history, basic settings (search engine, home page, clear data, cookie policy).
