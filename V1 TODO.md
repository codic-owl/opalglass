# V1 TODO

Updated: 2026-10-17. Package candidates were researched against their upstream
documentation; they have not been integrated or benchmarked in Opalglass.

Aim: a reliable, fast, cross-platform app for everyday Markdown notes. Finish
essential workflows first, then pick additions where existing code or a package
does most of the work. Full Obsidian parity is a separate milestone.

This tracks remaining work. Existing features and limitations are documented in
[README.md](README.md); the detailed rendering inventory is
[obsidian-coverage.json](tests/fixtures/obsidian-coverage.json).

## Priority 1 — Essential note workflows

- [ ] **Keep saved history accessible after renaming or moving a note.**
  Reuse the current history store, `serde`, `sha2` and atomic-write helpers.
  Add a durable history mapping or migration, including folder moves, destination
  collisions and interrupted operations. Old versions must remain available after
  restart, and reusing an old filename must not mix unrelated note histories.
  A new database is unnecessary for this task.

- [ ] **Make Editing Preview consistent with Reading.**
  Reuse `pulldown-cmark` and our shared rendering/source-mapping code. Share
  document context for multiline comments, reference-style links, footnotes and
  nested/foldable callouts. Finish footnote superscript typography. Preserve
  raw Markdown when editing, selecting, copying, undoing and composing text.
  This requires application work; another parser alone will not fix it.

- [ ] **Complete heading and block-reference navigation.**
  Reuse the parser and link resolver. Cover heading normalization and Setext
  headings, and resolve `[[note#^block-id]]` and same-note references. Navigate to
  targets beyond the opening preview and inside folded content. Missing targets
  should be visibly unresolved.

- [ ] **Embed notes, headings and blocks.**
  Reuse the renderer for `![[note]]`, `![[note#Heading]]` and
  `![[note#^block-id]]`. Build on reference navigation; resolve relative links from
  the embedded note, refresh changed sources, detect cycles and bound recursion.
  Load content on demand without retaining the whole vault in RAM.

- [ ] **Verify caret mapping and scroll synchronization for new formatting.**
  Extend existing GPUI interaction tests and native checks for Unicode,
  selection, clicks and editing around hidden Markdown syntax. Test both split
  panes and transitions between Source, Live Preview and Reading.

- [ ] **Bound rendering work for very large table cells.**
  Extend existing display chunking/virtualization. Preserve complete text,
  selection and copying while preventing a single cell from blocking a frame.

## Priority 2 — Low-hanging fruit

These are good candidates to interleave with the essential work. Effort estimates
are relative, not measured schedules.

| Task | Package / existing capability | Remaining work | Expected effort |
| --- | --- | --- | --- |
| Automatic URL and email detection | [linkify](https://docs.rs/linkify/latest/linkify/) | Apply to ordinary text only; exclude code and existing links. Check punctuation, Unicode and URL boundaries against Obsidian fixtures. | Small |
| Requested image dimensions | Existing link parser and GPUI layout | Interpret width / width×height syntax, preserve aspect ratio where appropriate, and fit narrow panes. | Small |
| Remote images | Existing GPUI URI image loader and HTTP-client interface | Configure a working client; provide loading/error states, request limits and bounded caches. Fetch asynchronously and avoid scroll jumps. | Small–medium |
| Markdown-link backlinks | Existing parsed reference data and backlink index | Include resolved Markdown/reference links in incoming backlinks; deduplicate and update after edits/moves. | Medium |

- [ ] Automatic URLs and email links.
- [ ] Requested image dimensions.
- [ ] Remote images.
- [ ] Markdown-link backlinks; current incoming backlinks are wiki-link based.

## Release gates — Start early, finish before claiming platform support

- [ ] **Run the configured native CI jobs successfully.**
  Linux, Windows, macOS Apple Silicon and Intel macOS. Run core/UI tests and
  build actual release packages. Configuration alone is not execution evidence.

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
