# Repository working rules

## Keep code small and navigable

- Aim for **200–500 lines per source or test file**. Treat 500 lines as a signal
  to review module boundaries; **do not grow a file beyond 1,000 lines**.
- Before adding functionality to a file over 500 lines, extract the relevant
  responsibility into a focused module. Existing large files are not a precedent
  for adding more unrelated code.
- Split by responsibility: search, navigation, persistence, rendering, input,
  dialogs and individual workflows should have clear homes. Keep coordinator
  files focused on shared state and wiring.
- Keep functions focused. Review functions approaching 100 lines for extraction,
  especially UI builders and event dispatchers. Give toolbar, pane, menu and
  dialog construction their own named helpers.
- Use ordinary Rust modules with narrow visibility. Do not use `include!` to
  disguise a monolithic production module or bypass formatting. Existing test
  and profiling wrappers may include application source to exercise the actual
  implementation without exposing a public testing API.
- Put substantial tests in separate files and group integration tests by feature.
- Do not split mechanically into numbered files or create tiny forwarding layers
  solely to meet a line count. Extract coherent responsibilities and keep their
  dependencies explicit.
- Check affected file sizes before finishing a change. If a file is nearing the
  limit, split it as part of the current work instead of leaving cleanup for later.

## Use context efficiently

- Start with `rg` and function/type outlines, then read the relevant ranges.
  Avoid dumping entire large files or unbounded build logs into context.
- Keep changes focused on the requested task. Preserve behavior during structural
  refactors and update test/evidence references when modules move.
- Run formatting, Clippy and the relevant existing tests after refactoring;
  include the UI harness when workspace/editor code moves. Keep all existing
  regression coverage, including ignored profiling tests.

## Preserve the app's priorities

- Keep note opening, tab changes and scrolling responsive. Do expensive I/O,
  parsing, indexing and rendering preparation off the UI thread; bound caches.
- Preserve cross-platform support and portable CPU defaults. Do not tune release
  builds for the developer's particular machine.
- Protect user notes: preserve conflict checks, recoverable edits and atomic
  no-overwrite behavior for moves and restores.
- Use [V1 TODO.md](<V1 TODO.md>) to prioritize essential note-taking workflows
  before optional parity features or additional code-editor functionality.
