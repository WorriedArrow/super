# javascript

## v8

- rusty_v8 (the `v8` crate), pinned to one version in the workspace. upgrades are their own pr.
- one isolate per renderer process. one context per realm: each same-process window, iframe and worklet gets its own.
- dedicated workers (m4) run on their own thread with their own isolate.
- a v8 startup snapshot with all bindings installed lands in m5 to cut renderer startup time.

## bindings (sp_webidl, sp_bindings)

- idl comes from w3c webref (the `@webref/idl` package), vendored into `crates/sp_webidl/idl/` at a pinned version. no hand-written idl unless webref lacks an interface.
- sp_webidl parses idl and generates rust at build time: v8 function templates, argument conversion per the webidl spec, overload resolution, exceptions, dictionaries, enums, callbacks, promises, iterables, [LegacyUnforgeable], [Exposed] and so on.
- each interface's behavior is a hand-written rust trait impl in sp_bindings, which the generated glue calls. interfaces that aren't implemented yet are still exposed, and their members throw NotSupportedError.

## dom and gc ownership

open question, first research issue of m2. the candidates:

- **a.** dom nodes stay in the rust arena. js wrappers are cppgc objects holding a document reference and a `NodeId`. the arena keeps a node alive while it's in a tree or while its wrapper is reachable, and cppgc tracing walks from wrappers into the dom.
- **b.** dom nodes are cppgc objects themselves, managed entirely by v8's heap, and the arena goes away.

the research has to cover cycle handling (node to listener closure to node), detached subtrees held only from js, cost of wrapper creation, and how servo, blink and ladybird do it. the outcome goes through a design-change issue to the owner.

## event loop

- the html spec's event loop: task queues per task source, microtask checkpoints, and "update the rendering" (resize, scroll, media queries, animations, fullscreen, rAF callbacks, style and layout, intersection and resize observers, paint).
- the v8 microtask policy is explicit, and sp_js runs checkpoints at exactly the spec's points.
- timers per the html timer initialization steps, including nesting clamping.
- rendering opportunities come from the compositor's vsync signal over ipc.

## modules and scripts

- classic scripts, es modules, import maps, dynamic import, module workers (m4).
- script fetching goes through the fetch spec in the network process like everything else.
