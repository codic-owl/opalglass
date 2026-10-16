# Opalglass

A native Markdown workspace built with Rust and GPUI. Notes are ordinary `.md`
files in a folder you choose. The editor, note model, and Markdown handling are
original implementations; no Zed editor or Obsidian application code is included.

This is an early desktop prototype targeting Linux only for now. Windows and
macOS are deferred. It is not yet an Obsidian replacement.

## Run

Use Rust **1.90.0** or newer. GPUI is pinned to a specific Apache-2.0 revision, and
the application's dependency versions are recorded in `Cargo.lock`.

```sh
cargo run --locked -- /path/to/your/vault
```

Without a path, the app reopens the last vault and its saved tabs. On first launch,
run `cargo run --locked` and choose **Open vault** (or press **Ctrl O**) to
browse for your vault folder. If the system folder picker is unavailable, a path
entry dialog appears instead. The selected directory must already exist.
The development profile optimizes the app and all runtime dependencies at
`opt-level = 2`. The first build after a profile change recompiles affected crates;
subsequent builds reuse them. Initial builds take longer, but native rendering
and tab interactions benefit. Development assertions and overflow checks remain
enabled. Release builds retain ThinLTO and one code generation unit.
Both profiles use Rust's default CPU baseline for the selected target, without
`target-cpu=native` or host-specific instruction flags. Each supported operating
system and architecture still needs its own build.

Try the sample vault without changing the included examples:

```sh
cp -R examples/vault /tmp/opalglass-demo
cargo run --locked -- /tmp/opalglass-demo
```

Build an optimized Linux executable:

```sh
cargo build --release --locked
./target/release/opalglass /path/to/your/vault
```

The executable is `target/release/opalglass`. It uses your system's desktop and
graphics libraries; it is not a standalone cross-distribution bundle.

### Linux prerequisites

GPUI needs an accessible X11 or Wayland desktop and a Vulkan-capable graphics
driver. On Ubuntu/Debian, install the build dependencies:

```sh
sudo apt-get install clang cmake pkg-config libssl-dev libfontconfig1-dev \
  libfreetype6-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libwayland-dev libvulkan-dev fonts-dejavu-core
```

Your distribution's Vulkan driver package is also needed at runtime. If a stale
`WAYLAND_DISPLAY` variable is set in an X11 session, launch with
`env -u WAYLAND_DISPLAY cargo run --locked -- /path/to/vault`.

## Included

- Collapsible, folders-first navigation, including empty folders; note creation
  (including nested folders), folder creation, and tabs.
- Per-vault sidebar expansion, scroll position, selection, width, and visibility.
- Keyboard tree navigation, full-path hover tooltips, and on-demand active-note reveal.
- Explorer context menus, inline naming, and drag-and-drop file/folder moves,
  with conflict checks, wiki-link updates, and open-tab migration.
- Draggable tabs with individual close buttons, middle-click closing, and keyboard switching.
- Original text editing with soft wrapping, mouse selection, clipboard,
  grapheme-aware deletion, undo/redo, and GPUI input-method integration.
- Source, split, and reading views with a native Markdown preview, icon controls
  with hover labels, and a draggable divider in split view. The reading pane’s
  lock button links scrolling to the editor using Markdown source positions;
  unlock it to scroll the two panes independently.
- Live Preview editing in Source and Split: use the pencil icon or Ctrl Shift E
  to toggle formatted Markdown. Inactive lines render headings, emphasis, links,
  quotes, lists, tasks, and code; the line being edited or selected shows its
  original syntax. Clipboard, saves, undo, and IME composition use the original
  Markdown. Rendering and source-to-display mappings are limited to viewport
  rows, with bounded caches and bounded parsing for exceptionally long lines.
- Source line numbers, active-line highlighting, word navigation/deletion, and
  page navigation with selection. Vertical movement remembers the text column.
- A centered reading column with clearer heading, quote, and link styling;
  complete code-block copying and an empty-note writing action.
- Wiki links, aliases, heading navigation, backlinks, and tag filtering.
  Unresolved wiki links use coral text and a wavy underline in source and preview;
  the styling updates when target notes are created, moved, or removed.
- Ranked Tantivy search over saved note titles, paths and contents, with typo
  tolerance and highlighted excerpts. ↑ / ↓ select a result; Enter opens it.
- Vault discovery that lists paths before reading note bodies, followed by
  cancellable background content/backlink indexing. Notes can be opened and
  edited while indexing runs; search results fill in automatically.
- Persistent search indexes outside the vault, incremental updates after saves,
  moves and external edits, and cancellable background queries. Excerpts load
  only for visible results, using bounded file reads and a small cache.
- Virtualized note navigation and indexed backlink lookup.
- Background note loading, reloading, and Markdown analysis. A new note shows
  its full editable source and a bounded opening preview before full analysis.
  Source and preview render only visible rows; long lines and blocks use bounded
  display chunks. Initial source indexing covers 128 display rows; the remaining
  row index is built in the background after paint.
- Cached preview, tags, word count, and dirty status; delta-based undo history
  and indexed Unicode cursor/IME positions.
- Atomic file replacement with external-change checks and a save-copy flow.
- Preservation of a note's line-ending style when saving edits.
- Background metadata checks every three seconds; unchanged note contents and
  parsed links are reused. Clean tabs reload external
  edits; dirty tabs stay intact and report a conflict.
- Word count and cursor position.

Autosave is enabled by default and checks changed notes every three seconds.
Writes run in the background with the same external-change checks as **Ctrl S**.
Edits made during a write remain dirty until the next save. Closing a tab/window,
quitting via Ctrl Q, or switching vaults saves dirty tabs and stops on a conflict.
An in-flight automatic save must finish before closing or moving files.

Unsaved snapshots are written durably outside the vault before automatic saves,
including when autosave is disabled in Settings. Reopening restores those edits
against their original disk baseline; external changes are never silently accepted
as the baseline for recovered text. Recovery covers the most recent completed
checkpoint, so edits since that checkpoint can still be lost in a crash.
**Reload** asks before discarding unsaved edits. **Save copy** preserves editor
contents separately when the original file conflicts.

## Everyday workflows

- **Find in note:** Ctrl F opens literal search with highlighted matches in Source,
  Live Preview, and Reading. Enter / Shift Enter or F3 / Shift F3 move through
  matches and wrap; Aa toggles case sensitivity. Ctrl P or Ctrl Shift F searches
  the whole vault.
- **Link completion:** type `[[` and a title, then use Up/Down and Enter/Tab, or
  click a suggestion. Duplicate names display their paths; inserted links use
  explicit vault-relative paths when necessary. Escape dismisses suggestions.
- **Editing:** Enter continues bullet, numbered, and task lists; new tasks start
  unchecked. Enter on an empty item exits the list. Tab / Shift Tab indent or
  outdent list items and selected lines. Shift Enter inserts a plain newline.
  Formatting shortcuts and commands preserve Markdown and undo history.
- **Outline:** the outline icon or Ctrl Shift O opens clickable headings. Selecting
  a heading jumps both the editor and reading pane to that section.
- **Resume work:** per-vault tabs, their order, the active note, selections, and
  editor/reading scroll positions are restored. Missing files are skipped unless
  unsaved recovery data exists. Alt Left/Right, toolbar arrows, and mouse side
  buttons navigate note history and restore positions; history lasts this session.
- **Command palette:** Ctrl Shift P or the terminal-shaped rail icon searches
  available actions. Use the keyboard or click a result.
- **Bookmarks:** Ctrl Shift D toggles the current note's bookmark. The bookmark
  rail icon opens the list. Bookmarks persist per vault and follow in-app moves.
- **Daily notes:** Ctrl Alt D or the calendar icon opens `Daily/YYYY-MM-DD.md`
  using the system's local date, creating it only when necessary. New daily notes
  use `Templates/Daily.md` when present; existing notes and open edits are preserved.
- **Templates:** place Markdown files under `Templates/`, then press Ctrl Alt T
  or choose Insert template in the palette. The selected template replaces the
  current selection as one undoable edit. Supported placeholders are `{{date}}`
  (YYYY-MM-DD), `{{time}}` (HH:MM), and `{{title}}` (current note name).
- **Settings:** the rail settings icon or Ctrl comma controls text size, source
  line numbers, the default Source/Live Preview editing mode, and autosave.
- **Tags:** click a `#tag` in Reading or in the connections panel; Ctrl-click it
  in Source/Live Preview. Search `tag:work` or `#work` to filter saved notes,
  including YAML tags and nested tags such as `work/research`. This filter takes
  the whole query; combining tags with text queries is not supported yet.
- **Properties:** use the note toolbar's Properties button or **Edit note
  properties** in the command palette. Add, rename, or remove YAML fields, or
  switch to the YAML editor for complex metadata. Apply is one undoable edit;
  invalid metadata leaves the note intact. Unrelated fields, comments, and the
  note body are preserved when editing supported field layouts.
- **Attachments:** paste a clipboard image with Ctrl V, or drop images/PDFs onto
  a note. Files are copied into the vault's `Attachments/` folder with unique
  names, and relative Markdown links are inserted at the caret. Reading displays
  standalone images; clicking an attachment opens the system viewer. Undo removes
  the inserted link but keeps the imported file. Moving notes between folders or
  moving attachment files currently requires updating these relative links.
- **External changes:** clean notes refresh automatically. For an edited note,
  **Compare versions** shows your text against disk, with an alternate view of
  disk changes since opening. Keep editing, load the disk version (undoable), or
  explicitly save your version over the reviewed disk contents. A second external
  edit requires a fresh comparison. A deleted note can be restored from its open
  copy. **Compare current note with disk** is also in the command palette.
- **Unlinked mentions:** choose **Find unlinked mentions of current note** in the
  palette. Review highlighted occurrences in saved notes, then click **Link** to
  convert one into a wiki link. Existing links, code, and YAML are excluded.
  Changed files and unsaved source tabs must be refreshed or saved first.
- **Replace across notes:** Ctrl Shift H opens literal workspace search/replace.
  Enter the search and replacement text, choose case sensitivity, and preview the
  changes. Review each file's diff and select which files to apply. Changing the
  query requires another preview. Selected files are checked before writing and
  again individually at save time; an error stops the operation and reports how
  many files were changed. This is not a multi-file transaction or a vault-wide
  undo operation. Unsaved selected notes must be saved first.

Right-click a file/folder and choose **Move to trash**, or press Delete with the
explorer focused. Affected dirty tabs save first; conflicts block deletion. Entire
folders, including attachments, move into the vault's hidden `.trash` directory.
Incoming links remain and become unresolved. **Restore last deleted item** in the
explorer menu restores the newest deletion to its original location, including
after restarting, and refuses to replace an existing destination. There is no
permanent-delete command or automatic trash expiry.

External-change checking uses the complete last-read file contents, including a
second check immediately before atomic replacement. It is optimistic concurrency,
not a lock shared with other editors; a concurrent writer can still race the final
check. Symlinks are skipped and rejected for note access.

### Search

Ordinary words match as AND across titles and bodies. Put the whole query in quotes
for an exact token phrase; quoted phrases never use typo correction. A trailing
`*` completes the last word, for example `gloss*`. Folder/path substring lookup is
also supported. Title matches rank above body matches: complete titles, whole
title words, final-word prefixes (at least three characters without `*`), title
substrings, then sparse typo corrections. Thus `ilia` puts `Iliad` above notes
containing `Ilia` only in their bodies, while `orphi` puts `Orphic` before
`Trimorphic`. Body results follow, with whole words before sparse prefix and
typo fallbacks. Known words are preserved in multiword typo correction. A
complete exact title does not need fuzzy fallback; quoted searches remain exact.

Results are limited to the top 100 (shown as `100+`); refine the query to narrow
them. Queries support up to eight indexed words / 512 UTF-8 bytes. Mixed quoted
clauses and advanced query operators are not supported yet. This is token search,
so its matches differ from the old arbitrary-substring body scan. Saved contents
are searched; unsaved edits enter the index after saving.

Results arrive before excerpts. Only visible rows request snippets, reading at
most 16 KiB from each original file. The index stores terms, positions, titles,
paths and sparse byte-offset checkpoints, not a retrievable copy of note bodies.
A vault scan retains only metadata, wiki links and content fingerprints; full
note text is held by open tabs or read temporarily for indexing and link edits.
A bounded snippet cache validates source metadata before reuse. In development
builds, the bottom bar records Search (including a separate backend phase) and
Search snippets timings.
Typing has a 16 ms debounce; superseded queries and excerpts are discarded.

On Linux the disposable search cache is under
`$XDG_CACHE_HOME/opalglass/search`, or `~/.cache/opalglass/search`. Cache paths are
also defined for Windows/macOS, although the desktop application currently targets
Linux. Caches stay outside the vault. An unavailable, locked or incompatible cache
falls back to a temporary session index. The app must finish its first index build
before full-text search is available. Notes remain editable during that build.
Indexing workers are created only for changed files and shut down after commits
and merges finish. The cache stays reserved without keeping a writer alive.
The app uses [mimalloc](https://github.com/microsoft/mimalloc) for Rust allocations
to reduce memory retained after indexing. This uses the target's normal CPU
baseline, with no machine-specific compiler flags or environment settings.
Saves and moves synchronize the index in the background; external changes are
picked up by the existing three-second refresh. A changed or missing source file
never reuses a stale excerpt. Unix ctime additionally detects same-length changes
with restored mtime. Other platforms use size and mtime.

To exercise search and scrolling in a real, read-only native window:

```sh
cargo run --release --locked --example native_search_profile -- /path/to/vault
```

This opens notes without editing them, waits for indexing, searches exact and
misspelled terms, scrolls results, and exits. Use a development build (omit
`--release`) to also print native first-paint timings.
Keep the benchmark window visible. Cache and sidebar-state files may be written
outside the vault; source notes are not changed. Backend tests use
`cargo test --locked -p opalglass-search`; UI regressions use the existing harness.

To audit backend memory independently of rendering:

```sh
cargo run --release --locked --example memory_profile -- /path/to/vault /tmp/opalglass-memory-cache
```

This reports live and peak Rust allocations after discovery, backlink scanning,
indexing and teardown, plus process RSS on Linux. The first run builds the index;
repeat with the same cache directory to measure reuse. Its allocation counter
wraps the same allocator as the app and is confined to the diagnostic executable.

## Sidebar

Folders start collapsed. Click a folder to expand or collapse it. The active note
is highlighted; following links leaves the tree alone. Clicking a note tab
expands its parents and scrolls it into view, showing the sidebar and clearing
search when needed. **Ctrl Shift B** also reveals the active note. Drag the sidebar's
right edge to resize it, or use **Ctrl B** / the panel icon in the thin left rail
to collapse or expand it. The rail stays accessible while the sidebar is hidden;
there is no separate top toolbar.

The file explorer has five icon buttons with hover labels:

- **New note** and **New folder** create at the vault root, with inline naming.
  The context menu and keyboard shortcuts can still create inside a selected folder.
- **Change sort order** offers name, modified time, and created time in either
  direction. Folders stay first. The menu supports Up/Down, Enter, and Escape.
- **Auto-reveal current file** toggles following the active note, including links
  and keyboard tab switches. It keeps document focus, search text, and the
  sidebar's collapsed state. It starts disabled; clicking a tab still reveals
  that note explicitly.
- **Collapse all** closes the folders; when all are closed, it becomes **Expand all**.

Sort order and auto-reveal are saved separately for each vault. Date sorting uses
cached filesystem metadata collected by background scanning. Files with unavailable
timestamps sort last, with names breaking ties; filesystems without creation times
use name order for that group. No filesystem reads occur while sorting on the UI
thread. Toolbar actions are disabled until a vault is open.

Search displays a ranked list with full relative paths and highlighted excerpts. Clearing the search restores
the tree and its scroll position. Both lists are virtualized; expansion traverses
cached folder entries and does no filesystem reads or Markdown parsing. In the
simulated UI, expanding every folder and scrolling to the last entry in the
4,425-note test vault took about 13 ms; ten idle redraws took about 77 ms total.
These are diagnostic CPU timings, not native GPU frame measurements.

Right-click a folder for **New note**, **New folder**, **Rename…**, or
**Move folder to…**. Right-click a note for **Rename…** or **Move file to…**.
Right-click empty explorer space to create at the vault root.
Names are edited directly in the tree: Enter confirms, Escape cancels. Creation
shortcuts use the selected folder, or the selected note's parent.

Drag a note or folder onto another folder to move it. Valid destinations
highlight during dragging. A folder
cannot be dropped into itself or its descendants. The **Move to…** menu action
opens a searchable list of existing destination folders; `/` denotes the vault
root. Use Up/Down and Enter, or click a destination. Moving preserves the item's
name; use Rename to change it.

With the tree focused, Up/Down select entries, Right expands or enters a folder,
Left collapses or selects its parent, Home/End jump, and Enter opens a note or
toggles a folder. Type a name to jump to a visible match; the prefix resets after
800 ms. **F2** renames the selection in the tree, or the active note when editing.
**Shift F10** opens the selected entry's menu; Up/Down and Enter choose an action.
**Ctrl Shift B** focuses the tree; clicking a tab keeps focus in the document.

Tabs activate on mouse press and reveal the note in the tree on click. Drag tabs to reorder them; the highlighted edge
shows the insertion position, and the dragged note remains active.
Each tab has a close button, and middle-click also closes a tab. Closing saves
that tab first; a save conflict leaves it open and focuses it. Closing a
background tab preserves the active document.

While indexing, the bottom bar shows **Indexing vault** with a compact progress
bar. Hover to see the note count and current stage. It remains visible through
the final commits and merges, then disappears when indexing finishes; cancelling
or switching vaults removes the previous job's indicator. Progress updates only
redraw the footer.

In development builds (`cargo run`), the bottom bar also shows the latest
action's time in milliseconds: opening notes or
vaults, switching/reordering/closing tabs, saving edits, and changing views.
Hover over the timing to see the last 20 entries and the latest action's phase
breakdown; click it to copy the full log with paths and phases for every entry.
Opening reports worker queue, reading, preparation, UI dispatch, UI installation,
frame wait, and rendering separately. Failed opens/saves and blocked closes are
labeled explicitly.
Timings run from the action handler to UI paint, including asynchronous loading
and saving where applicable. They exclude GPU presentation and later background
Markdown analysis or vault indexing. The history stays in memory for this session.
Release builds omit the performance widget and its log history.

These interactions follow Obsidian's documented
[file explorer](https://obsidian.md/help/plugins/file-explorer),
[note management](https://obsidian.md/help/manage-notes), and
[tab navigation](https://obsidian.md/help/tabs) patterns for the supported actions.
Ctrl N, F2, Ctrl Tab / Ctrl Shift Tab, and Ctrl 1–9 match those documented actions.
Ctrl Shift N for folder creation and Shift F10 for the explorer menu are
Opalglass shortcuts; they are not claimed as Obsidian defaults.

Rename and move save open edits first and stop on conflicts. They run in the
background while a progress overlay prevents edits or closing the window. Resolved
wiki links preserve their targets, aliases, and headings, including relative links
inside moved notes. If a local name shadows the intended target, a leading `/`
in an updated wiki link explicitly selects the vault root. Code examples and
unresolved links are left as written.
Standard Markdown links, attachment references, and external applications' caches
are not rewritten. Tabs whose contents change are reloaded, resetting their undo
history; other tabs retain their buffers. Existing destinations are never replaced.
Failed moves attempt to roll back link edits without overwriting concurrent changes
and report any incomplete rollback. Multi-file moves are not crash-atomic.

Workspace state, preferences, bookmarks, and recovery records are stored outside
the vault. On Linux they live under
`$XDG_STATE_HOME/opalglass/navigation`, falling back to
`~/.local/state/opalglass/navigation`. Each canonical vault path has its own state.
Workspace state is saved during the periodic refresh and on normal close or vault
switch. Windows/macOS state paths use LocalAppData/Application Support respectively;
those desktop targets remain unverified.

## Shortcuts

| Shortcut | Action |
| --- | --- |
| Ctrl O | Browse for a vault folder |
| Ctrl N | New note in selected folder |
| Ctrl Shift N | New folder in selected folder |
| Ctrl B | Hide / show sidebar |
| Ctrl Shift B | Reveal active note and focus tree |
| F2 | Rename tree selection / active note |
| Shift F10 (tree focused) | Open context menu |
| Ctrl P / Ctrl Shift F | Focus vault search |
| Ctrl F | Find in current note |
| Enter / Shift Enter (find focused), F3 / Shift F3 | Next / previous match |
| Ctrl Shift P | Command palette |
| Ctrl Shift O | Toggle heading outline |
| Alt Left / Alt Right | Navigate back / forward |
| Ctrl Shift D | Toggle bookmark |
| Ctrl Alt D | Open today's daily note |
| Ctrl Alt T | Insert template |
| Ctrl comma | Settings |
| Delete (tree focused) | Move selected entry to recoverable trash |
| Ctrl Alt B / Ctrl Alt I / Ctrl Alt C | Bold / italic / inline code |
| Ctrl Enter | Toggle task checkbox |
| Tab / Shift Tab | Indent / outdent list item or selected lines |
| Ctrl S | Save active note |
| Ctrl Shift S | Save a copy |
| Ctrl W | Save and close active tab |
| Ctrl Tab / Ctrl Shift Tab | Next / previous tab |
| Ctrl 1–8 / Ctrl 9 | Select numbered tab / last tab |
| Ctrl E | Cycle source / split / reading |
| Ctrl Shift E | Toggle raw Source / Live Preview editing |
| Ctrl Q | Save all and quit |
| Ctrl A / C / X / V | Select all / copy / cut / paste |
| Ctrl Z / Ctrl Shift Z / Ctrl Y | Undo / redo |
| Shift + arrows | Extend selection |
| Ctrl Left / Ctrl Right | Move by word or punctuation segment |
| Ctrl Shift Left / Ctrl Shift Right | Extend selection by word |
| Ctrl Backspace / Ctrl Delete | Delete previous / next word, or selection |
| Page Up / Page Down | Move by one viewport in source |
| Shift Page Up / Shift Page Down | Extend selection by one viewport |
| Ctrl Home / Ctrl End | Start / end of note |
| Escape | Cancel inline naming, dismiss menu/dialog, or cancel drag |

## Verification

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

The additional GPUI test-platform harness exercises input dispatch, editing,
clipboard, saving, note creation, link navigation, search, conflict retention,
folder actions, context menus, inline naming, drag-and-drop moves, tab ordering and
closing, keyboard navigation, resize/hide, and per-vault state restoration:

```sh
cargo test --manifest-path ui-tests/Cargo.toml
```

It lives in a separate package because GPUI's test support adds dependencies not
needed by the application. First use requires network access to download them.
The CI workflow builds the release executable and runs both test suites on Linux.
A workflow file is not evidence of a successful CI run.

For the provided offline development cache, an adapter can run the same tests:

```sh
python3 scripts/offline_ui_tests.py
```

It copies GPUI into `target/` and changes only its temporary build manifest to
omit `util/test-support`, an unused Git-repository fixture dependency. GPUI and
application Rust source remain unchanged. Normal CI uses GPUI's original
manifest. These tests use GPUI's simulated window and text system, not a GPU.

Verified in the development sandbox: Linux compilation, core unit tests, GPUI
interaction tests using the offline adapter, and Clippy. Native launch was
attempted, but the sandbox cannot connect to the advertised Wayland/X11 display
and exposes no GPU device. The unmodified UI harness is blocked by missing
offline dependencies. Native visual behavior and actual platform IME input
remain unverified.

Before calling this ready for daily use, run the UI harness and then manually:

1. Open a temporary copy of the sample vault and resize the window.
2. Edit, select, paste, undo, save, close, and reopen a Unicode note.
3. Follow a wiki link and its backlink; search for a note and a tag.
4. Change a clean note in another editor and verify it reloads.
5. Change a dirty note externally and verify Save does not overwrite it.
6. Save a copy, reload the original, then close the app.
7. Check a real IME composition and clipboard exchange in a Linux desktop session.
8. Right-click files, folders, and empty explorer space. Create and rename inline,
   drag files/folders to folders, move to `/` through the folder chooser, reorder tabs, and close a
   dirty background tab. Check cancellation and conflicts.
9. Check source line numbers, wrapped-line movement, and Page Up/Down at narrow
   and wide sizes. In reading mode, check the centered column and copy a long code block.

## Current boundaries

The original Markdown parser supports common block and inline forms, but is not
a complete CommonMark implementation. Tables, inline image layout, nested list
layout, reference links, math, footnotes, and HTML are not implemented. Inline
code and fenced code remain literal. Standard HTTP(S) and mail links open in the
system browser on click; wiki links open local notes. Ambiguous bare note titles
need an explicit vault-relative path.

Search and incoming backlinks use saved contents. Opening a vault first scans
file/folder names and builds the link resolver, without reading note bodies.
The tree becomes available after that scan; opening a note reads that note on
demand. Content and backlinks are indexed in the background, with progress shown
in the status bar. Until the first search index is ready, search matches paths, then automatically
refreshes with ranked body matches. Reopening reuses the search index and checks
source metadata; unchanged notes are not indexed again. Enter on a pending
search waits for matching results. Existing links resolve from the path inventory,
so links are not marked missing just because their contents haven't been indexed.

Saving, creating, or moving a note supersedes stale indexing work. Switching
vaults cancels the previous scan/index task, and partial or failed index results
never replace the current snapshot. Rename/move still finishes a complete scan
before updating wiki links. Later refreshes read only changed files. Search uses
a persistent Tantivy index. The vault model discards bodies after extracting
links and a SHA-256 fingerprint; moves read affected notes on demand and verify
that the fingerprint still matches before applying cached link ranges.
Search indexing uses up to two Tantivy workers on machines with at least four
available CPUs, and one on smaller machines. Workers share a 32 MiB indexing
budget; one merge worker and 16 MiB source batches keep memory bounded. This is
an indexing-buffer budget, not a limit on total application RAM. All indexing
workers are released after each update. A release backend benchmark on the
4,425-note / 353 MiB vault averaged 12.4 s with two workers versus 14.8 s with
one (two fresh-index runs each, warm filesystem cache). Four workers took 12.4 s
in one run and used more memory. Backlink scanning adds
about one second; these timings exclude UI startup. Reproduce fresh indexing
timings with `cargo run --release --example indexing_profile -- /path/to/vault`;
the example only reads the vault and removes its temporary search index on exit.
Search results are reused
until the query or vault changes. Notes use a contiguous text buffer, so inserting near
the start of a large document still moves its suffix. Display row metadata is
rebuilt after edits, but text shaping and preview layout are limited to visible
content. Opening parses at most the first 8 KiB for its first preview and starts
full analysis after the first paint. It also indexes only the first 128 source
display rows initially, including for very long physical lines. The remaining
source index is built in the background and appended without replacing measured
viewport rows or changing the cursor and scroll position. Immediate jumps beyond
the opening rows complete the index synchronously; edits discard stale results.
The full file is still read and the editable text buffer initialized before
opening, so disk I/O and buffer initialization still scale with file size.
Code copying waits for a complete preview
so it cannot silently copy a truncated block. Heading navigation beyond the
opening excerpt completes when analysis is ready, and scroll position is retained.
Analysis after edits is debounced by 75 ms; the preview and word count may briefly
show the previous revision while it runs. Stale results are discarded. Leaving a
partially analyzed note cancels its initial analysis until that tab is active again.
Undo retains up to 200 edits within a 32 MiB changed-text budget, always retaining
the latest edit even if it exceeds that budget. Durable save/conflict checks
still complete synchronously before close or quit. There is
no Obsidian plugin compatibility, graph view, sync,
or attachment library management yet. Templates use simple substitutions; there is no
scripting engine or custom date-format syntax.

Read-only performance measurements for a vault:

```sh
cargo run --release --locked --example vault_profile -- /path/to/vault
OPALGLASS_PROFILE_VAULT=/path/to/vault python3 scripts/offline_ui_tests.py \
  profile_vault_redraws -- --ignored --nocapture
```

The first command reports path discovery, background content/backlink indexing,
unchanged refresh, and search separately. The second reports when the vault,
first note, and full index become available in the simulated GPUI interface,
plus redraws and scrolling. It uses the development profile and does not measure
GPU frames.

In the October 14 startup check, a generated vault of 4,000 notes (47.4 MB, with
repeated wiki links) took 2.09 s to load before these changes. Path discovery now
takes about 11 ms, and content/backlink indexing takes another 0.98 s in the
background. Duplicate targets within a note are resolved once when building
backlinks. These are single-run storage-layer timings in a debug build with a
warm filesystem cache; they exclude first-note parsing and native rendering,
and are not a guarantee for other vaults or storage devices.

To profile note opening, scrolling in Source/Reading/Split, redraws, editing,
tab switches, and saving:

```sh
OPALGLASS_PROFILE_NOTE=/path/to/large-note.md python3 scripts/offline_ui_tests.py \
  profile_note_work -- --ignored --nocapture
```

This reads the specified note and operates on a temporary copy. Omit the
environment variable for a reproducible 252 KB Markdown fixture. The measurements
use the same simulated platform and optimized app/GPUI development profile as
the UI tests, not a native GPU session. The profiler reports first source paint
separately from completion of full Markdown analysis.

Measured in that harness during the October 13 performance audit:

| Operation | 252 KB fixture before | 252 KB fixture after | 4.65 MB / 116,343-line note after |
| --- | ---: | ---: | ---: |
| Open note, including preparation | 2,527 ms | 97 ms | 795 ms |
| One redraw (average of three) | 2,609 ms | 7 ms | 6 ms |
| Jump to end and insert | 5,222 ms | 18 ms | 40 ms |

Loading and analysis run on background tasks. Cached tab switches averaged
5–7 ms; the large note's background preview update took 633 ms. These timings
are diagnostic samples, not frame-rate guarantees. For native measurements,
use the desktop benchmark below.

Scrolling uses independently cached source, preview, explorer, and connection
views. Incoming links and tags are displayed in a virtual list: offscreen links
do not create buttons or participate in layout. The UI regression tests check
that scrolling either text pane leaves the other views cached, for both short
and long documents, and that the last backlink and tag remain reachable.

To stress the connections panel with a generated vault (all writes are temporary):

```sh
OPALGLASS_PROFILE_BACKLINKS=2000 python3 scripts/offline_ui_tests.py \
  profile_note_work -- --ignored --nocapture
```

In the October 14 debug simulated UI run, the 252 KB fixture with 2,000 incoming
links took about 412 ms to open and 155–159 ms per scroll frame before these
changes. Afterward, opening took about 107 ms and scrolling took about 5–6 ms
per frame across Source, Reading, and both Split panes. These measurements
exclude native GPU/compositor costs and vary by machine. The default fixture
has no extra incoming links; `OPALGLASS_PROFILE_NOTE` also accepts a short note.

After separating first paint from full analysis and optimizing the app/GPUI
development builds, the latest October 14 opening measurements are:

| Generated fixture | First source paint | Full analysis finished |
| --- | ---: | ---: |
| 57 bytes, 2,000 backlinks | 3.5 ms | 3.8 ms |
| 252 KB, 2,000 backlinks | 6.3 ms | 17.1 ms |
| 4.66 MB, 296,000 lines | 48.9 ms | 196 ms |

These runs include reading the note from a warm local filesystem, creating its
editable buffer, and drawing the first text in the simulated window. They are
diagnostic samples, not a promise of a native frame rate. Both the staging and
build optimization changed since the older measurements above. The normal
`cargo run --locked -- /path/to/vault` command uses these optimizations.
Regression tests cover first paint before bulk analysis, typing during opening,
late heading navigation, and preventing truncated code copies.

Tab transitions cache the cursor's line/column and each note's connection list,
including its scroll position. Selecting an already active tab skips redraws.
Saving an existing note updates only its changed outgoing backlink edges instead
of rebuilding the whole vault index. Conflict checks still run before closing.

To measure switching, opening an already active tab, and clean/dirty closing:

```sh
OPALGLASS_PROFILE_NOTE=/path/to/note.md OPALGLASS_PROFILE_BACKLINKS=2000 \
  python3 scripts/offline_ui_tests.py profile_tab_work -- --ignored --nocapture
```

In the October 14 simulated UI run, three 660 KB single-line Unicode notes with
2,000 backlinks switched in about 2–2.5 ms each across Source/Split/Reading,
down from 12–14 ms. Closing a clean background tab took 1.2 ms, down from 12 ms;
saving and closing a dirty tab took 5.5 ms. These are warm local filesystem
samples; native GPU/compositor latency is not included.

The simulated platform does not exercise Linux's real font shaping/rasterization
stack. To test an actual desktop window with the native renderer and GPU:

```sh
OPALGLASS_PROFILE_BACKLINKS=2000 cargo run --locked --example native_profile
```

This opens a separate benchmark window, creates a temporary vault with two short
notes and one note containing varied paragraphs, exercises opens/closes/switches,
prints phase timings, and exits. Keep the window visible while it runs.
Avoid interacting with the benchmark window until it exits.
`OPALGLASS_PROFILE_NOTE=/path/to/note.md` uses a temporary copy of that note as
the larger fixture. No original notes are edited.

An October 14 native Wayland/KDE run on Intel MTL graphics reproduced a 159 ms
open and 82–97 ms tab switches with varied text and 2,000 backlinks. Rendering
accounted for 150 ms of that open; disk reading and preparation took under 1 ms.
The development profile had optimized GPUI but left Cosmic Text, Rustybuzz,
TTF parsing and Taffy unoptimized. Optimizing those dependencies reduced the
same native workload to a 36 ms open (23 ms rendering) and 18 ms switches
(about 4 ms rendering). These native samples include compositor frame scheduling
but still stop before GPU presentation; they supersede simulated latency claims.
The final build also optimizes Swash glyph rasterization. A follow-up with the
302 KB varied-text fixture measured a 34 ms open and 13 ms tab switches; short
notes opened in 8–14 ms. The native window was 950 × 1140 logical pixels.

After moving the remaining source-row indexing behind the first paint, a 6.04 MB
varied-text note with 2,000 backlinks opened in 31.5 ms instead of 155 ms on the
same native window size. First-frame rendering fell from 130.7 ms to 18.0 ms;
reading and initial buffer/preview preparation still took about 7 ms. The full
source row index finishes asynchronously after the note is already visible.

A subsequent portable-build comparison used the same 6.04 MB fixture and 2,000
backlinks at 950 × 1020 logical pixels on the same native Wayland desktop. These
are medians from two complete runs per variant, with a warm filesystem cache;
incomplete runs and runs at a different viewport were excluded:

| Build | Open 6 MB note | Opening render phase | Switch to 6 MB note |
| --- | ---: | ---: | ---: |
| Selected development dependencies optimized | 28.9 ms | 17.3 ms | 16.6 ms |
| All runtime dependencies optimized (new development default) | 22.2 ms | 9.9 ms | 13.1 ms |
| Release, ThinLTO, one code generation unit | 21.7 ms | 5.8 ms | 10.8 ms |

Each run includes one large-note opening and two switches to that note. Total
latency includes variable compositor frame scheduling; the render phase measures
CPU work and excludes GPU presentation. These small Linux samples support the
development profile change, but do not establish results on other platforms.
No CPU-specific flags were used in any variant. Cargo's
[profile documentation](https://doc.rust-lang.org/cargo/reference/profiles.html)
describes these optimization settings and their build-time tradeoffs.

## Layout and licensing

- `src/buffer.rs`: Unicode text, selection, history, and composition model.
- `src/editing.rs`: Markdown editing transactions and literal in-note matching.
- `src/commands.rs`: command catalog, daily notes, and template substitutions.
- `src/state.rs`: preferences and durable unsaved recovery records.
- `src/document.rs`: normalization and preservation of line endings.
- `src/vault.rs`: vault storage, paths, link resolution, conflict checks.
- `crates/search`: persistent Tantivy index, ranking, typo correction and snippets.
- `src/markdown.rs`: original parser and link/tag extraction.
- `src/prepared.rs`: cached preview rows, formatting spans, tags, and word counts.
- `src/properties.rs`: YAML frontmatter parsing and comment-preserving field edits.
- `src/attachments.rs`: collision-safe imports and portable attachment links.
- `src/changes.rs`: saved-note replacement plans and unlinked mentions.
- `src/comparison.rs`: bounded-time diffs for external edits and replacement previews.
- `src/editor.rs`: GPUI text rendering, hit testing, keyboard and IME bridge.
- `src/workspace.rs`: desktop interface, tabs, dialogs, and vault refresh.

Opalglass code is MIT-licensed. GPUI and the Zed utility crates it pulls in are
Apache-2.0; they are separate from Zed's GPL editor. Other dependencies keep
their own licenses. Generate a dependency inventory and collected notices before
distributing binaries:

```sh
python3 scripts/dependency_notices.py --offline
```

The script writes `target/THIRD_PARTY_NOTICES.txt` for the host platform. Review
its list of missing license files before packaging. It does not replace a full
release license audit, particularly for a different platform.
