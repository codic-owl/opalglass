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

Or run `cargo run --locked` and choose **Open vault**. A folder picker and a path
entry dialog are both available. The selected directory must already exist.

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
- File/folder rename and move with conflict checks, wiki-link updates, and open-tab migration.
- Original text editing with soft wrapping, mouse selection, clipboard,
  grapheme-aware deletion, undo/redo, and GPUI input-method integration.
- Source, split, and reading views with a native Markdown preview.
- Source line numbers, active-line highlighting, word navigation/deletion, and
  page navigation with selection. Vertical movement remembers the text column.
- A centered reading column with clearer heading, quote, and link styling;
  complete code-block copying and an empty-note writing action.
- Wiki links, aliases, heading navigation, backlinks, and tag filtering.
- Search over saved note paths and contents; Enter opens the first result.
- Background vault loading and debounced, cancellable full-text search.
- Virtualized note navigation and indexed backlink lookup.
- Background note loading, reloading, and Markdown analysis. Source and preview
  render only visible rows; long lines and blocks use bounded display chunks.
- Cached preview, tags, word count, and dirty status; delta-based undo history
  and indexed Unicode cursor/IME positions.
- Atomic file replacement with external-change checks and a save-copy flow.
- Preservation of a note's line-ending style when saving edits.
- Background metadata checks every three seconds; unchanged note contents and
  parsed links are reused. Clean tabs reload external
  edits; dirty tabs stay intact and report a conflict.
- Word count and cursor position.

Saving is explicit with **Ctrl S**. Closing a tab/window, quitting via Ctrl Q,
or switching vaults attempts to save dirty tabs and stops if a save fails.
There is no crash recovery or automatic periodic saving yet. **Reload** asks
before discarding unsaved edits. **Save copy** preserves the editor contents in a
new file; a conflicting original tab still needs to be reloaded or resolved.

External-change checking uses the complete last-read file contents, including a
second check immediately before atomic replacement. It is optimistic concurrency,
not a lock shared with other editors; a concurrent writer can still race the final
check. Symlinks are skipped and rejected for note access.

## Sidebar

Folders start collapsed. Click a folder to expand or collapse it. The active note
is highlighted; following links leaves the tree alone. **Reveal active note**
(**Ctrl Shift B**) expands its parents and scrolls it into view. Drag the sidebar's
right edge to resize it, or use **Ctrl B** / the **☰** button to hide or show it.

Search displays a flat list with full relative paths. Clearing the search restores
the tree and its scroll position. Both lists are virtualized; expansion traverses
cached folder entries and does no filesystem reads or Markdown parsing. In the
simulated UI, expanding every folder and scrolling to the last entry in the
4,425-note test vault took about 13 ms; ten idle redraws took about 77 ms total.
These are diagnostic CPU timings, not native GPU frame measurements.

Select a folder, then use **+ New** or **+ Folder** to create inside it. With a file
selected, creation uses its parent. **Select vault root** resets that destination.
**Rename** changes the selected entry's name; **Move** takes a full vault-relative
destination, including the file/folder name. The destination parent must exist.
Right-clicking an entry selects it without opening it, so the toolbar can act on it.

With the tree focused, Up/Down select entries, Right expands or enters a folder,
Left collapses or selects its parent, Home/End jump, and Enter opens a note or
toggles a folder. Type a name to jump to a visible match; the prefix resets after
800 ms. **F2** renames the selection. Reveal active note also focuses the tree.

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

Sidebar state is stored outside the vault under
`$XDG_STATE_HOME/opalglass/navigation`, falling back to
`~/.local/state/opalglass/navigation`. Each canonical vault path has its own state.
It is saved during the periodic refresh and on normal close or vault switch.

## Shortcuts

| Shortcut | Action |
| --- | --- |
| Ctrl O | Open vault by path |
| Ctrl N | New note in selected folder |
| Ctrl Shift N | New folder in selected folder |
| Ctrl B | Hide / show sidebar |
| Ctrl Shift B | Reveal active note and focus tree |
| F2 (tree focused) | Rename selection |
| Ctrl P / Ctrl F | Focus vault search |
| Ctrl S | Save active note |
| Ctrl Shift S | Save a copy |
| Ctrl W | Save and close active tab |
| Ctrl E | Cycle source / split / reading |
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
| Escape | Dismiss dialog |

## Verification

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

The additional GPUI test-platform harness exercises input dispatch, editing,
clipboard, saving, note creation, link navigation, search, conflict retention,
folder actions, keyboard navigation, resize/hide, and per-vault state restoration:

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
8. Check source line numbers, wrapped-line movement, and Page Up/Down at narrow
   and wide sizes. In reading mode, check the centered column and copy a long code block.

## Current boundaries

The original Markdown parser supports common block and inline forms, but is not
a complete CommonMark implementation. Tables, image rendering, nested list
layout, reference links, math, footnotes, and HTML are not implemented. Inline
code and fenced code remain literal. Standard HTTP(S) and mail links open in the
system browser on click; wiki links open local notes. Ambiguous bare note titles
need an explicit vault-relative path.

Search and incoming backlinks use saved contents. Initial loading reads all note
contents into memory on a background thread; later scans read only changed files.
Search results are reused until the query or vault changes. Full-text search still
scans the in-memory text. Notes use a contiguous text buffer, so inserting near
the start of a large document still moves its suffix. Display row metadata is
rebuilt after edits, but text shaping and preview layout are limited to visible
content. Markdown analysis is debounced by 75 ms; the preview and word count may
briefly show the previous revision while it runs. Stale results are discarded.
Undo retains up to 200 edits within a 32 MiB changed-text budget, always retaining
the latest edit even if it exceeds that budget. Durable save/conflict checks
still complete synchronously before close or quit. There is
no Obsidian plugin compatibility, graph view, live-preview editing, sync,
attachment management, rename/delete UI, or session persistence yet.

Read-only performance measurements for a vault:

```sh
cargo run --release --locked --example vault_profile -- /path/to/vault
OPALGLASS_PROFILE_VAULT=/path/to/vault python3 scripts/offline_ui_tests.py \
  profile_vault_redraws -- --ignored --nocapture
```

The second command measures the simulated GPUI interface in a debug build,
including scrolling to the end of the note list; it does not measure GPU frames.

To profile note opening, redraws, editing, tab switches, and saving:

```sh
OPALGLASS_PROFILE_NOTE=/path/to/large-note.md python3 scripts/offline_ui_tests.py \
  profile_note_work -- --ignored --nocapture
```

This reads the specified note and operates on a temporary copy. Omit the
environment variable for a reproducible 252 KB Markdown fixture. The measurements
use the same debug simulated platform as the UI tests, not a native GPU session.

Measured in that harness during the October 13 performance audit:

| Operation | 252 KB fixture before | 252 KB fixture after | 4.65 MB / 116,343-line note after |
| --- | ---: | ---: | ---: |
| Open note, including preparation | 2,527 ms | 97 ms | 795 ms |
| One redraw (average of three) | 2,609 ms | 7 ms | 6 ms |
| Jump to end and insert | 5,222 ms | 18 ms | 40 ms |

Loading and analysis run on background tasks. Cached tab switches averaged
5–7 ms; the large note's background preview update took 633 ms. These timings
are diagnostic samples, not frame-rate guarantees. Native GPU behavior still
needs verification on a Linux desktop.

## Layout and licensing

- `src/buffer.rs`: Unicode text, selection, history, and composition model.
- `src/document.rs`: normalization and preservation of line endings.
- `src/vault.rs`: vault storage, paths, search, link resolution, conflict checks.
- `src/markdown.rs`: original parser and link/tag extraction.
- `src/prepared.rs`: cached preview rows, formatting spans, tags, and word counts.
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
