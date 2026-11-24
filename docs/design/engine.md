# engine

the renderer-side pipeline from bytes to display list.

## html (sp_html)

- tokenizer and tree construction exactly as the html spec defines them, including the adoption agency algorithm, foster parenting, template contents, and foreign content (svg, mathml).
- encoding sniffing per the encoding spec. decoding uses encoding_rs.
- the parser supports script interleaving from the start: parser-blocking scripts, `document.write` re-entering the tokenizer, and pausing for pending stylesheets. the hooks exist in m1 and m2 wires them to js.
- a speculative preload scanner arrives in m3.
- gate: 100% of html5lib-tests tokenizer and tree-construction cases (scripting-on cases from m2).

## dom (sp_dom)

- one arena per document. nodes are addressed by `NodeId`, a generational index. no `Rc<RefCell<Node>>` trees.
- tree mutation goes through the dom spec's algorithms (insert, pre-insert, remove, replace, adopt) so mutation observers, live collections, range updates and custom element reactions all hang off one place.
- events: the dom spec's dispatch with full path computation, including shadow-root retargeting once shadow dom exists (m4).
- how node lifetime ties into the js garbage collector is decided by research in m2 (see js.md). until then nothing outside sp_dom holds a `NodeId` across a task.

## css (sp_css)

- syntax per css syntax level 3, a tokenizer and parser producing component values.
- properties come from `crates/sp_css/properties.toml` (name, inherited, initial value, grammar, computed value rule, animation type, logical group). a build script generates the property enum, parsers and computed value types. adding a property is one toml entry plus whatever value-type parsing it needs.
- selectors level 4, matching right to left, bloom filter for ancestor checks. `:has()` in m4.
- at-rules: @media, @supports, @import, @font-face, @layer, @keyframes, @container (m4), @property (m4).
- custom properties and var() substitution at computed-value time, with cycle detection per spec.

## style (sp_style)

- cascade with origins, layers, specificity, importance, scoping (m4) and inline styles. user-agent stylesheet is the html spec's rendering section, kept as a .css file.
- computed values stored per element, sharing identical structs (arc'd groups of properties, similar to servo's style structs).
- invalidation tracks ids, classes, attributes and states per selector so a change restyles only what it could affect. m1 can restyle everything, but the api has to allow incremental from day 0.
- parallel styling with rayon from m3.

## text (sp_text)

- system font enumeration and fallback through fontique. web fonts (woff, woff2) load in the renderer from bytes the network fetched.
- shaping with harfrust, glyph data with skrifa.
- line breaking and grapheme and word segmentation through icu4x. bidi through icu4x or unicode-bidi (researcher picks in m1).
- inline layout itself (line boxes, justification, vertical-align, white-space handling, text-transform) belongs to sp_layout, not to a text layout library.

## layout (sp_layout)

- styled dom to box tree to fragment tree. the box tree follows css display: anonymous boxes, display contents, list markers, generated content.
- code works in logical coordinates (inline and block axes) from day 0, even though only horizontal-tb is supported in v0.
- formatting contexts: block (margin collapsing, clearance, floats), inline (line breaking, bidi reordering, baselines), positioned (absolute, fixed, sticky), flex, grid, table, and multicol after v0.
- intrinsic sizing (min-content, max-content, fit-content) shared by every formatting context.
- incremental relayout from dirty bits lands in m3. m1 lays out the whole document each time.
- fragment tree is immutable once produced. hit testing, scrolling geometry, getBoundingClientRect and painting all read it.

## paint (sp_paint)

- builds stacking contexts and walks them in css 2 appendix e painting order.
- produces a display list: rects, rounded rects, borders, glyph runs (font, glyph ids, positions), images, box shadows, gradients, clips, transforms, opacity, filters, blend modes, and scroll frames.
- display list items reference fonts and images by id. the compositor gets the bytes once and caches them.

## compositor (sp_compositor, browser process)

- receives display lists per frame, keeps a layer tree, and builds one vello scene per window frame including the chrome.
- scrolling happens in the compositor without a round trip to the renderer. the renderer learns the new offset afterward.
- transform and opacity animations run in the compositor from m3.
- presentation goes through wgpu at the display refresh rate, and only when something changed.
