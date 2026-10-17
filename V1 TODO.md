# V1 TODO

Updated: 2026-10-17. Package candidates were researched against their upstream
documentation. Checked items are implemented and tested locally; unchecked
packages and features remain candidates.

Aim: a reliable, fast, cross-platform app for everyday Markdown notes. Finish
essential workflows first, then pick additions where existing code or a package
does most of the work. Full Obsidian parity is a separate milestone.

This tracks remaining work. Existing features and limitations are documented in
[README.md](README.md); the detailed rendering inventory is
[obsidian-coverage.json](tests/fixtures/obsidian-coverage.json).

## Priority 1 — Essential note workflows

- [x] **Keep saved history accessible after renaming or moving a note.**
  Implemented durable identity routing and interrupted-move recovery using the
  existing history store, `serde`, `sha2` and atomic-write helpers. Tests cover
  folder moves, collisions, restart, interrupted operations and isolated histories
  when an old filename is reused. Overlapping captures keep the selected note
  identity for duplicate detection and pruning. UI restoration and undo also pass.

- [x] **Make Editing Preview consistent with Reading.**
  Reuse `pulldown-cmark` and our shared rendering/source-mapping code. Share
  document context for multiline comments, reference-style links, footnotes and
  nested/foldable callouts and footnote superscript typography. Preserve
  raw Markdown when editing, selecting, copying, undoing and composing text.
  This requires application work; another parser alone will not fix it.
  Shared context now handles multiline comments, reference-style links,
  numbered definitions/backlinks and smaller raised footnote references. Nested
  callout widgets fold actual virtual rows; hidden comment/reference/unused
  definition lines compact without removing genuine blank lines. Core and UI
  tests cover Unicode mappings, clicks, copy, undo, nested definitions, explicit
  navigation into folds and simulated composition. Representative native Linux
  input/layout checks also pass; exact Obsidian typography and full visual
  parity are not established.

- [x] **Complete heading and block-reference navigation.**
  Parser-derived metadata covers formatted/Setext headings, normalized and
  hierarchical heading links, block IDs and same-note references. UI tests verify
  targets beyond the opening preview, folded callouts, missing-target styling,
  saved/unsaved target changes and cancellation after user navigation. Target
  lookups use metadata; rendering does not read note files.

- [x] **Embed notes, headings and blocks.**
  Reuse the renderer for `![[note]]`, `![[note#Heading]]` and
  `![[note#^block-id]]`. Build on reference navigation; resolve relative links from
  the embedded note, refresh changed sources, detect cycles and bound recursion.
  Load content on demand without retaining the whole vault in RAM.
  Implemented in Reading/Split with background preparation, compact virtual row
  composition and a bounded cache. Links, images, tasks, footnotes and code-copy
  actions use the source note; saved and inactive unsaved edits refresh embeds.
  Tests cover complete-context selection, nesting, repeated/same-note embeds,
  cycles/depth limits, Unicode, idle/cache-pressure leases, cancellation and
  scroll anchors. Native Linux whole/section/block rendering and actions pass.
  Recognized block-ID markers hide without changing editable/copied source,
  including duplicate IDs while navigation keeps the first target. Footnote
  returns preserve their destination when the source note opens asynchronously;
  changed sources and newer navigation reject stale destinations.

- [x] **Verify caret mapping and scroll synchronization for new formatting.**
  Extend existing GPUI interaction tests and native checks for Unicode,
  selection, clicks and editing around hidden Markdown syntax. Test both split
  panes and transitions between Source, Live Preview and Reading.
  New editor tests cover compact rows, independent nested folds, physical saved
  scroll positions, visual-column movement and simulated UTF-16 composition.
  A mixed-format Workspace test now covers locked split scrolling, Source/Live/
  Reading transitions, folded-target navigation and unlocked independence. It
  also caught and fixed a queued Reading correction overriding newer navigation.
  The recorded development build passes native Linux checks in an isolated KWin
  compositor:
  Unicode caret input/save/undo, exact source copy, mode transitions, long-note
  locked keyboard scrolling, callout folds and embedded navigation. Its binary
  hash and screenshots are recorded in `tests/fixtures/v1-verification.json`.
  The subsequent audit fixes pass current automated regressions; the recorded
  native run predates those fixes.
  The user's normal KDE automation context still leaves the test window
  unmapped; real IME and other native platforms remain release gates below.

- [x] **Bound rendering work for very large table cells.**
  Reading now virtualizes cell display slices and shares complete text/styles
  across them. Normal slices are about 1 KiB; exceptional graphemes are capped
  at 4 KiB per slice without changing source text. Parser and UI tests cover
  Unicode, styles, footnotes/callouts, reaching the tail, clicking links in later
  slices, and full source selection/copy in Source and Editing Preview.

## Priority 2 — Low-hanging fruit

These are good candidates to interleave with the essential work. Effort estimates
are relative, not measured schedules.

| Task | Package / existing capability | Remaining work | Expected effort |
| --- | --- | --- | --- |
| Automatic URL and email detection | [linkify](https://docs.rs/linkify/latest/linkify/) | Implemented shared text detection, code/link exclusions, source mapping and boundary tests. | Complete locally |
| Requested image dimensions | Existing link parser and GPUI layout | Implemented requested width / width×height, aspect ratios and narrow-pane sizing. | Complete locally |
| Remote images | Pinned `reqwest`, `image`, `resvg/usvg` and a bounded GPUI image cache | Implemented background downloads/decoding, limits, loading/errors/retry and scroll-anchor preservation; native release checks remain below. | Complete locally |
| Markdown-link backlinks | Existing parsed reference data and backlink index | Implemented resolved Markdown/reference backlinks with deduplication and updates after edits/moves. | Complete locally |

- [x] Automatic URLs and email links via `linkify`, shared by Reading and Editing
  Preview. Tests cover punctuation, Unicode paths, entities, highlighting, source
  positions and mouse clicks; code and existing links retain their behavior.
- [x] Requested image dimensions for local image widgets in Reading/Split.
  Wiki, Markdown and reference-image labels accept width or width×height.
  Width-only uses intrinsic proportions; narrow panes scale proportionally.
  Parser/source-map and UI geometry tests pass. Table-cell image widgets and
  exact inline image flow remain deferred parity work.
- [x] Remote images in Reading/Split, with background HTTP(S) fetch/decode,
  loading/error states and retry. Two global load/decode slots, a bounded queue,
  16 MiB downloads and a 32 MiB decoded cache protect responsiveness and memory.
  Tests cover redirects, cancellation, actual/declared oversize bodies, cache
  eviction, dimensions, scroll anchors and a real loopback download through the UI.
  Animated GIF/WebP show their first frame; SVG has stricter resource limits and
  rejects filters. Nested SVG image resources and exact inline image flow remain
  outside the current supported subset.
- [x] Markdown-link backlinks, including used reference-style links. Tests cover
  deduplication, edits, moves, missing-target creation and removal; unused
  definitions, external URLs and self-links are excluded.

## Release gates — Start early, finish before claiming platform support

- [ ] **Run the configured native CI jobs successfully.**
  Linux, Windows, macOS Apple Silicon and Intel macOS. Run core/UI tests and
  build actual release packages. Configuration alone is not execution evidence.
  Successful runs have been reported for Linux and macOS Apple Silicon.
  Windows reached packaging; its notices encoding fix and a successful Intel
  macOS run remain to be confirmed.

- [ ] **Validate packaged applications on native desktops.**
  Cover launch, folder picker, editing, clipboard, drag/drop, save/conflicts,
  rename/history, recovery and state/cache paths. Check macOS shortcut conventions
  and filesystem case behavior. Complete signing/notarization and installation
  checks for the intended public distribution. Existing packaging scripts are
  sufficient to start; no additional packaging crate is required yet.

- [ ] **Test real IME composition.**
  Use actual composing input methods on Linux, Windows and macOS. Verify marked
  text, candidate placement, commit/cancel, selection, undo and switching tabs.
  Reuse GPUI's input integration; simulated UTF-16 tests are already present but
  do not replace native engine tests.

- [ ] **Expose and test screen-reader semantics.**
  Evaluate a GPUI upgrade using its newer
  [AccessKit integration](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/_accessibility.rs).
  Our current pin lacks that integration. Expose control names, roles, focus,
  editor text, selection and actions; test with NVDA, VoiceOver and Orca.
  [AccessKit](https://github.com/AccessKit/accesskit) supplies platform adapters,
  but rich-text/hypertext support has limitations. Check upgrade regressions and
  document any remaining accessibility limitations explicitly.

- [ ] **Repeat release performance and memory checks after additions.**
  Measure vault indexing/reopening, short and long note opening, tab changes,
  scrolling and editing. Include notes containing the new features. Preserve
  portable CPU settings, background processing and bounded caches; release
  builds must continue to omit the development performance widget.

- [ ] **Finish the v1 compatibility audit.**
  Compare supported fixtures in actual Reading and Live Preview, including
  interaction and source preservation. Update the inventory and README with
  evidence and remaining limitations. Passing structural tests is not proof of
  visual or behavioral equivalence to Obsidian.

## Optional v1 additions — Prototype before committing to scope

Packages handle substantial rendering work here. Accept an addition only after a
small prototype demonstrates useful coverage, portable builds, acceptable cold
start/RAM and straightforward integration. These should not delay the essentials.

| Feature | Candidate | What the package handles | What remains / decision |
| --- | --- | --- | --- |
| Math | [mathjax-svg-rs](https://docs.rs/mathjax-svg-rs/latest/mathjax_svg_rs/) | MathJax TeX-to-SVG using embedded QuickJS and a worker thread. | Parse inline/display math, place baselines, theme/cache output and preserve editable source. Check supported syntax and platform dependencies. First math candidate to prototype. |
| Mermaid | [mermaid-rs-renderer](https://github.com/1jehuang/mermaid-rs-renderer) | Native diagram parsing/layout and SVG output; optional vector-scene API. | Compare syntax/layout with our Obsidian fixtures, handle errors and cache background output. Upstream speed claims are not Opalglass measurements. First diagram candidate to prototype. |
| Audio embeds | [rodio](https://github.com/RustAudio/rodio) | Decoding/playback through CPAL and codec support. | Add play/pause/seek controls and resource cleanup; verify formats and native audio dependencies. Reconsider a separate audio backend if adopting GStreamer for video. |
| Basic embedded search | Existing Tantivy engine and snippet system | Indexed search, ranking and excerpts. | Render a bounded embedded results list, refresh after index updates, and define supported query syntax. Full Obsidian search-language compatibility is additional work. |

- [ ] Prototype math and record compatibility/performance results.
- [ ] Prototype Mermaid and record compatibility/performance results.
- [ ] Assess audio playback integration and packaging cost.
- [ ] Assess basic embedded search using our existing supported queries.

## Deferred — Broader rendering parity

| Remaining feature | Packages / foundation | Why deferred |
| --- | --- | --- |
| HTML rendering | [html5ever](https://docs.rs/html5ever/latest/html5ever/) for parsing; [ammonia](https://docs.rs/ammonia/latest/ammonia/) if sanitization is needed | Parsing does not provide native layout. Start with an explicitly supported tag subset. [Blitz](https://github.com/DioxusLabs/blitz) offers HTML/CSS rendering but is beta and requires substantial integration. |
| Video embeds | [gstreamer](https://docs.rs/gstreamer/latest/gstreamer/) and related bindings | Native runtime/plugins, frame presentation, codecs and cross-platform packaging are significant work. Could also supply audio. |
| PDF embeds, page selection and height | [pdfium-render](https://github.com/ajrcarey/pdfium-render) | Broad rendering foundation, but Pdfium must be packaged separately. Add visible-page rendering and bounded caches. [hayro](https://github.com/LaurenzV/hayro) is a pure-Rust alternative that remains experimental with performance work outstanding. |
| Canvas embeds | Existing `serde` / `serde_json`, GPUI and the [JSON Canvas specification](https://jsoncanvas.org/spec/1.0/) | Parsing is straightforward; node/edge rendering, referenced content and viewer interactions are application work. Start with a read-only preview. |
| Complete embedded-search compatibility | Existing Tantivy plus an application query parser | Obsidian operators, mixed clauses, tag/text combinations and result presentation require explicit semantics and tests. |
| Exact image flow and fine typography | Existing renderer | Inline image placement, unusual highlight delimiter runs, exact callout icons/themes and whitespace differences remain parity polish. |
| Exhaustive syntax-highlighting parity | Existing Syntect | Current highlighting is sufficient for note-taking. Matching every Prism grammar/theme is low priority; unknown languages already remain readable. |

- [ ] HTML subset, followed by a separate decision about broader CSS support.
- [ ] Video embeds.
- [ ] PDF embeds with page/height parameters.
- [ ] Canvas previews.
- [ ] Advanced search operators and embedded-query compatibility.
- [ ] Remaining image-flow, delimiter and typography differences.
- [ ] Exhaustive code-language parity only if a concrete note-taking need arises.

## Later product roadmap — Separate from the v1 rendering checklist

- [ ] Graph view.
- [ ] Sync.
- [ ] Modules/plugins; Obsidian plugin compatibility would be a separate project.
- [ ] Attachment library management beyond current import/move/trash workflows.
- [ ] Richer template substitutions/date formats if needed; scripting is deferred.

Package choices for these broader features need their own requirements and
research. They should not expand the current v1 scope by default.

## Suggested execution order

1. Preserve history through renames; start native release/accessibility work early.
2. Add URL detection, image dimensions and remote images as contained quick wins.
3. Share document context across previews; finish references and note embeds.
4. Finish caret/large-cell checks and Markdown backlinks; validate essentials.
5. Pick successful math/Mermaid prototypes, then consider audio or embedded search.
6. Complete release verification and publish an accurate supported-feature list.

For every new package, verify the selected version's license/notices, supported
Rust version and platform build requirements. Initialize expensive renderers on
demand, keep rendering off the UI thread, and measure cache/resource lifetimes.
