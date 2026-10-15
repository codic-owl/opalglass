#![allow(dead_code)]

// The GPUI test platform has extra dependencies, so the harness is a separate
// package: the application and storage tests also build with a minimal cache.
mod editor {
    include!("../../src/editor.rs");

    #[cfg(test)]
    mod tests {
        use super::{Editor, LINE};
        use gpui::{EntityInputHandler, Modifiers, TestAppContext, point, px};

        #[gpui::test]
        fn word_shortcuts_and_page_selection(cx: &mut TestAppContext) {
            let text = format!(
                "café e\u{301}lan\n{}",
                "A longer line of text\n".repeat(200)
            );
            let (view, cx) = cx.add_window_view(|window, cx| {
                let editor = Editor::new(&text, false, "", cx);
                editor.focus.focus(window);
                editor
            });
            cx.simulate_keystrokes("ctrl-right ctrl-shift-right");
            view.read_with(cx, |editor, _| {
                assert_eq!(editor.buffer.selected(), " e\u{301}lan")
            });
            cx.simulate_keystrokes("ctrl-backspace ctrl-z");
            view.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), text));
            cx.simulate_keystrokes("ctrl-home shift-pagedown");
            view.read_with(cx, |editor, _| {
                assert!(editor.buffer.line_column().0 > 5);
                assert_eq!(editor.buffer.anchor(), 0);
                assert!(!editor.buffer.selected().is_empty());
                let caret = editor.point_for(editor.buffer.cursor());
                assert!(caret.y >= editor.bounds.top() && caret.y < editor.bounds.bottom());
            });
            cx.simulate_keystrokes("pageup");
            view.read_with(cx, |editor, _| assert_eq!(editor.buffer.cursor(), 0));
        }

        #[gpui::test]
        fn vertical_movement_retains_column_across_short_lines(cx: &mut TestAppContext) {
            let (view, cx) = cx.add_window_view(|window, cx| {
                let editor = Editor::new("abcdefghij\nx\nabcdefghij", false, "", cx);
                editor.focus.focus(window);
                editor
            });
            cx.simulate_keystrokes("right right right right right down down");
            view.read_with(cx, |editor, _| {
                assert_eq!(editor.buffer.line_column(), (3, 6))
            });
            cx.simulate_keystrokes("up up");
            view.read_with(cx, |editor, _| {
                assert_eq!(editor.buffer.line_column(), (1, 6))
            });
        }

        #[gpui::test]
        fn large_source_shapes_only_visible_rows_and_navigates_beyond_viewport(
            cx: &mut TestAppContext,
        ) {
            let text = "line 😀\n".repeat(20_000);
            let (view, cx) = cx.add_window_view(|window, cx| {
                let editor = Editor::new(&text, false, "", cx);
                editor.focus.focus(window);
                editor
            });
            view.read_with(cx, |editor, _| {
                assert!(editor.source_ranges.len() > 20_000);
                assert!(editor.cached_rows.len() < 100);
            });
            cx.simulate_keystrokes(&vec!["down"; 60].join(" "));
            view.read_with(cx, |editor, _| {
                assert_eq!(editor.buffer.line_column().0, 61)
            });
            cx.simulate_keystrokes("ctrl-end");
            view.read_with(cx, |editor, _| {
                assert_eq!(editor.buffer.cursor(), text.len());
                let point = editor.point_for(text.len());
                assert!(point.y >= editor.bounds.top() && point.y < editor.bounds.bottom());
                assert!(
                    editor
                        .visible_rows
                        .iter()
                        .any(|r| r.range.end == text.len())
                );
                assert!(editor.cached_rows.len() <= 256);
            });
            cx.simulate_input("ş");
            cx.simulate_keystrokes("ctrl-z");
            view.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), text));
        }

        #[gpui::test]
        fn long_physical_line_keeps_caret_and_unicode_selection_visible(cx: &mut TestAppContext) {
            let text = "a😀e\u{301} ".repeat(4000);
            let (view, cx) = cx.add_window_view(|window, cx| {
                let editor = Editor::new(&text, false, "", cx);
                editor.focus.focus(window);
                editor
            });
            cx.simulate_keystrokes("ctrl-end shift-left shift-left");
            view.read_with(cx, |editor, _| {
                assert_eq!(editor.buffer.selected(), "e\u{301} ");
                let point = editor.point_for(editor.buffer.cursor());
                assert!(point.y >= editor.bounds.top() && point.y < editor.bounds.bottom());
                assert!(editor.cached_rows.iter().all(|r| r.range.len() < 300));
            });
        }

        #[gpui::test]
        fn mouse_selection_and_utf16_composition(cx: &mut TestAppContext) {
            let (view, cx) = cx.add_window_view(|window, cx| {
                let editor = Editor::new("Alpha 😀 beta", false, "", cx);
                editor.focus.focus(window);
                editor
            });
            let position = view.read_with(cx, |editor, _| {
                assert!(!editor.visible_rows.is_empty());
                editor.point_for(6) + point(px(0.), px(LINE / 2.))
            });
            cx.simulate_click(position, Modifiers::default());
            cx.simulate_keystrokes("shift-right");
            view.read_with(cx, |editor, _| assert_eq!(editor.buffer.selected(), "😀"));
            view.update_in(cx, |editor, window, cx| {
                // UTF-16 positions 6..8 cover one emoji, not two UTF-8 bytes.
                editor.replace_and_mark_text_in_range(Some(6..8), "ni", Some(0..2), window, cx);
                assert_eq!(editor.buffer.text(), "Alpha ni beta");
                assert_eq!(editor.marked_text_range(window, cx), Some(6..8));
                editor.replace_text_in_range(None, "你", window, cx);
                assert_eq!(editor.buffer.text(), "Alpha 你 beta");
            });
            cx.simulate_keystrokes("ctrl-z");
            view.read_with(cx, |editor, _| {
                assert_eq!(editor.buffer.text(), "Alpha 😀 beta")
            });
        }

        #[gpui::test]
        fn long_single_line_field_keeps_caret_visible(cx: &mut TestAppContext) {
            let (view, cx) = cx.add_window_view(|window, cx| {
                let editor = Editor::new("", true, "", cx);
                editor.focus.focus(window);
                editor
            });
            let long = "a/".repeat(200);
            // Paste is intentionally one transaction.
            view.update_in(cx, |editor, window, cx| {
                editor.replace_text_in_range(None, &long, window, cx)
            });
            cx.run_until_parked();
            view.read_with(cx, |editor, _| {
                assert_eq!(editor.buffer.text(), long);
                assert!(editor.scroll_x > px(0.));
                assert!(editor.point_for(editor.buffer.cursor()).x < editor.bounds.right());
            });
            cx.simulate_keystrokes("home");
            view.read_with(cx, |editor, _| assert_eq!(editor.scroll_x, px(0.)));
        }
    }
}
#[path = "../../src/theme.rs"]
mod theme;
mod workspace {
    include!("../../src/workspace.rs");

    #[cfg(test)]
    mod tests {
        use super::{PathBuf, Workspace};
        use gpui::TestAppContext;

        #[gpui::test]
        fn discovered_vault_is_editable_and_pending_search_completes_after_indexing(
            cx: &mut TestAppContext,
        ) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("A.md"), "[[B]] actual content").unwrap();
            std::fs::write(folder.path().join("B.md"), "needle-only-in-body").unwrap();
            let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(None, w, cx));
            view.update_in(cx, |this, w, cx| {
                let vault = opalglass::vault::Vault::discover(
                    folder.path(),
                    &std::sync::atomic::AtomicBool::new(false),
                )
                .unwrap();
                this.finish_open(Ok((vault, Default::default())), w, cx);
                // Hold the background phase so the test can interact with the
                // independently usable path inventory before indexing finishes.
                this.index_revision = this.revision;
            });
            cx.run_until_parked();
            view.read_with(cx, |this, cx| {
                assert_eq!(this.tree.visible.len(), 2);
                assert!(!this.vault.as_ref().unwrap().content_indexed());
                assert!(
                    this.vault
                        .as_ref()
                        .unwrap()
                        .notes
                        .iter()
                        .all(|n| n.text.is_empty())
                );
                assert_eq!(
                    this.tabs[this.active].editor.read(cx).buffer.text(),
                    "[[B]] actual content"
                );
                assert!(!this.link_highlight("B").underline.unwrap().wavy);
            });
            cx.simulate_keystrokes("ctrl-end");
            cx.simulate_input(" edited");
            cx.simulate_keystrokes("ctrl-p");
            cx.simulate_input("needle-only-in-body");
            cx.simulate_keystrokes("enter");
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(150));
            cx.run_until_parked();
            view.read_with(cx, |this, _| {
                assert!(this.search_results.is_empty());
                assert!(this.open_search_result);
            });
            view.update(cx, |this, cx| {
                this.index_revision = u64::MAX;
                cx.notify();
            });
            cx.run_until_parked();
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(150));
            cx.run_until_parked();
            view.read_with(cx, |this, cx| {
                assert!(this.vault.as_ref().unwrap().content_indexed());
                assert_eq!(this.tabs[this.active].path, PathBuf::from("B.md"));
                let a = this
                    .tabs
                    .iter()
                    .find(|t| t.path == PathBuf::from("A.md"))
                    .unwrap();
                assert_eq!(
                    a.editor.read(cx).buffer.text(),
                    "[[B]] actual content edited"
                );
                assert!(a.dirty(cx));
                assert!(!this.error, "{}", this.status);
            });
        }

        #[gpui::test]
        fn indexing_restarts_after_save_and_is_cancelled_on_vault_switch(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            let other = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("A.md"), "old").unwrap();
            std::fs::write(other.path().join("Other.md"), "other").unwrap();
            let (view, cx) =
                cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
            let cancelled = view.update(cx, |this, cx| {
                this.vault = Some(
                    opalglass::vault::Vault::discover(
                        folder.path(),
                        &std::sync::atomic::AtomicBool::new(false),
                    )
                    .unwrap(),
                );
                this.revision += 1;
                this.update_indexing(cx);
                let cancelled = this.index_cancelled.clone();
                this.tabs[0].editor.update(cx, |editor, cx| {
                    editor.set_text("saved during indexing", cx)
                });
                assert!(this.save_tab(0, cx));
                cancelled
            });
            cx.run_until_parked();
            assert!(cancelled.load(std::sync::atomic::Ordering::Relaxed));
            view.read_with(cx, |this, cx| {
                assert!(this.vault.as_ref().unwrap().content_indexed());
                assert_eq!(
                    this.vault.as_ref().unwrap().notes[0].text,
                    "saved during indexing"
                );
                assert_eq!(
                    this.tabs[0].editor.read(cx).buffer.text(),
                    "saved during indexing"
                );
                assert!(!this.tabs[0].dirty(cx));
            });
            let cancelled = view.update_in(cx, |this, w, cx| {
                this.vault = Some(
                    opalglass::vault::Vault::discover(
                        folder.path(),
                        &std::sync::atomic::AtomicBool::new(false),
                    )
                    .unwrap(),
                );
                this.revision += 1;
                this.update_indexing(cx);
                let cancelled = this.index_cancelled.clone();
                this.open_vault(other.path().into(), w, cx);
                cancelled
            });
            cx.run_until_parked();
            assert!(cancelled.load(std::sync::atomic::Ordering::Relaxed));
            view.read_with(cx, |this, _| {
                assert_eq!(this.vault.as_ref().unwrap().root, other.path());
                assert_eq!(this.tabs[0].path, PathBuf::from("Other.md"));
                assert!(this.vault.as_ref().unwrap().content_indexed());
            });
        }

        #[gpui::test]
        fn missing_note_links_update_after_vault_changes_and_edits(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(folder.path().join("Folder")).unwrap();
            let source = "# Note\n[[Known]] [[Later#Heading|未作成]] [[#Note]] `[[Code]]`\n\n```\n[[Fenced]]\n```\n[Website](https://example.com)";
            std::fs::write(folder.path().join("Folder/Note.md"), source).unwrap();
            std::fs::write(folder.path().join("Folder/Known.md"), "known").unwrap();
            let (view, cx) =
                cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
            view.update_in(cx, |this, w, cx| {
                this.open_note("Folder/Note.md".into(), w, cx)
            });
            cx.run_until_parked();
            view.read_with(cx, |this, cx| {
                let editor = this.tabs[this.active].editor.read(cx);
                assert_eq!(editor.source_links.len(), 3); // Code examples are excluded.
                assert_eq!(editor.missing_links.len(), 1);
                assert_eq!(
                    &editor.buffer.text()[editor.missing_links[0].clone()],
                    "[[Later#Heading|未作成]]"
                );
                let missing = this.link_highlight("Later#Heading");
                assert!(missing.underline.unwrap().wavy);
                assert_eq!(
                    missing.color,
                    Some(gpui::rgb(crate::theme::MISSING_LINK).into())
                );
                for target in [
                    "Known",
                    "#Note",
                    "https://example.com",
                    "mailto:a@example.com",
                ] {
                    assert!(
                        !this.link_highlight(target).underline.unwrap().wavy,
                        "{target}"
                    );
                    assert_ne!(this.link_highlight(target).color, missing.color);
                }
            });
            // File creation fixes the link without editing or reopening its source.
            std::fs::write(folder.path().join("Folder/Later.md"), "# Heading").unwrap();
            view.update(cx, |this, cx| {
                let vault = opalglass::vault::Vault::open(folder.path()).unwrap();
                this.apply_scan(vault, this.revision, cx);
            });
            cx.run_until_parked();
            view.read_with(cx, |this, cx| {
                assert!(
                    this.tabs[this.active]
                        .editor
                        .read(cx)
                        .missing_links
                        .is_empty()
                );
                assert!(!this.link_highlight("Later#Heading").underline.unwrap().wavy);
            });
            // Removing a target invalidates the source rendering cache too.
            std::fs::remove_file(folder.path().join("Folder/Known.md")).unwrap();
            view.update(cx, |this, cx| {
                let vault = opalglass::vault::Vault::open(folder.path()).unwrap();
                this.apply_scan(vault, this.revision, cx);
            });
            cx.run_until_parked();
            view.read_with(cx, |this, cx| {
                let editor = this.tabs[this.active].editor.read(cx);
                assert_eq!(editor.missing_links.len(), 1);
                assert_eq!(
                    &editor.buffer.text()[editor.missing_links[0].clone()],
                    "[[Known]]"
                );
                assert!(this.link_highlight("Known").underline.unwrap().wavy);
            });
            cx.simulate_keystrokes("ctrl-a");
            cx.simulate_input("[[Later]] [[New missing]]");
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(75));
            cx.run_until_parked();
            view.read_with(cx, |this, cx| {
                let editor = this.tabs[this.active].editor.read(cx);
                assert_eq!(editor.missing_links.len(), 1);
                assert_eq!(
                    &editor.buffer.text()[editor.missing_links[0].clone()],
                    "[[New missing]]"
                );
            });
        }

        fn right_click(cx: &mut gpui::VisualTestContext, selector: &'static str) {
            let point = cx.debug_bounds(selector).unwrap().center();
            cx.simulate_mouse_down(point, gpui::MouseButton::Right, gpui::Modifiers::default());
            cx.simulate_mouse_up(point, gpui::MouseButton::Right, gpui::Modifiers::default());
        }
        fn click(cx: &mut gpui::VisualTestContext, selector: &'static str) {
            let point = cx.debug_bounds(selector).unwrap().center();
            cx.simulate_click(point, gpui::Modifiers::default());
        }
        fn drag(cx: &mut gpui::VisualTestContext, source: &'static str, target: &'static str) {
            let from = cx.debug_bounds(source).unwrap().center();
            let to = cx.debug_bounds(target).unwrap().center();
            cx.simulate_mouse_down(from, gpui::MouseButton::Left, gpui::Modifiers::default());
            cx.simulate_mouse_move(
                from + gpui::point(gpui::px(8.), gpui::px(0.)),
                gpui::MouseButton::Left,
                gpui::Modifiers::default(),
            );
            cx.simulate_mouse_move(to, gpui::MouseButton::Left, gpui::Modifiers::default());
            cx.simulate_mouse_up(to, gpui::MouseButton::Left, gpui::Modifiers::default());
            cx.run_until_parked();
        }

        #[gpui::test]
        fn explorer_context_creation_inline_rename_and_root(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::create_dir(folder.path().join("Ideas")).unwrap();
            std::fs::write(folder.path().join("Welcome.md"), "[[Ideas/Draft]]").unwrap();
            let (view, cx) =
                cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
            right_click(cx, "entry:Ideas");
            view.read_with(cx, |this, _| {
                assert_eq!(this.tabs[this.active].path, PathBuf::from("Welcome.md"));
                assert_eq!(
                    this.explorer_menu.as_ref().unwrap().target,
                    Some("Ideas".into())
                );
            });
            click(cx, "explorer-action-1"); // New folder inside the clicked folder.
            assert!(cx.debug_bounds("entry-name").is_some());
            cx.simulate_input("Nested");
            cx.simulate_keystrokes("enter");
            assert!(folder.path().join("Ideas/Nested").is_dir());
            right_click(cx, "entry:Ideas");
            cx.simulate_keystrokes("enter"); // New note, chosen with the keyboard.
            cx.simulate_input("Draft");
            cx.simulate_keystrokes("enter");
            assert!(folder.path().join("Ideas/Draft.md").exists());
            cx.simulate_keystrokes("f2"); // Active note, with the editor focused.
            view.read_with(cx, |this, cx| {
                assert_eq!(this.input.read(cx).buffer.selected(), "Draft")
            });
            cx.simulate_input("Renamed");
            cx.simulate_keystrokes("enter");
            assert!(!folder.path().join("Ideas/Draft.md").exists());
            assert_eq!(
                std::fs::read_to_string(folder.path().join("Welcome.md")).unwrap(),
                "[[Ideas/Renamed]]"
            );
            right_click(cx, "vault-location");
            click(cx, "explorer-action-0");
            cx.simulate_input("Root note");
            cx.simulate_keystrokes("enter");
            assert!(folder.path().join("Root note.md").exists());
            // Empty explorer space offers root creation and the menu stays on screen.
            let bounds = cx.debug_bounds("explorer").unwrap();
            let point = gpui::point(
                bounds.left() + gpui::px(12.),
                bounds.bottom() - gpui::px(8.),
            );
            cx.simulate_mouse_down(point, gpui::MouseButton::Right, gpui::Modifiers::default());
            cx.simulate_mouse_up(point, gpui::MouseButton::Right, gpui::Modifiers::default());
            view.read_with(cx, |this, _| {
                assert!(this.explorer_menu.as_ref().unwrap().target.is_none())
            });
            cx.simulate_keystrokes("escape");
            view.read_with(cx, |this, _| assert!(this.explorer_menu.is_none()));
            cx.simulate_keystrokes("ctrl-shift-n");
            cx.simulate_input("Cancelled");
            cx.simulate_keystrokes("escape");
            assert!(!folder.path().join("Cancelled").exists());
        }

        #[gpui::test]
        fn explorer_drag_moves_links_and_rejects_recursive_drop(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(folder.path().join("Ideas/Nested")).unwrap();
            std::fs::create_dir(folder.path().join("Archive")).unwrap();
            std::fs::write(folder.path().join("Ideas/Note.md"), "body").unwrap();
            std::fs::write(folder.path().join("Welcome.md"), "[[Ideas/Note]]").unwrap();
            let (view, cx) =
                cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
            click(cx, "entry:Ideas");
            click(cx, "entry:Ideas/Note.md");
            cx.simulate_keystrokes("ctrl-end");
            cx.simulate_input(" edited");
            drag(cx, "entry:Ideas/Note.md", "entry:Archive");
            assert_eq!(
                std::fs::read_to_string(folder.path().join("Archive/Note.md")).unwrap(),
                "body edited"
            );
            assert_eq!(
                std::fs::read_to_string(folder.path().join("Welcome.md")).unwrap(),
                "[[Archive/Note]]"
            );
            view.read_with(cx, |this, _| {
                assert_eq!(
                    this.tabs[this.active].path,
                    PathBuf::from("Archive/Note.md")
                )
            });
            drag(cx, "entry:Ideas", "entry:Ideas/Nested");
            assert!(folder.path().join("Ideas/Nested").is_dir());
            assert!(!folder.path().join("Ideas/Nested/Ideas").exists());
            drag(cx, "entry:Archive/Note.md", "vault-location");
            assert!(folder.path().join("Note.md").exists());
            assert_eq!(
                std::fs::read_to_string(folder.path().join("Welcome.md")).unwrap(),
                "[[Note]]"
            );
            // Move through the searchable folder chooser as well.
            right_click(cx, "entry:Note.md");
            click(cx, "explorer-action-1");
            cx.simulate_input("Nested");
            view.read_with(cx, |this, _| {
                assert_eq!(this.move_results, vec![PathBuf::from("Ideas/Nested")])
            });
            cx.simulate_keystrokes("enter");
            assert!(folder.path().join("Ideas/Nested/Note.md").exists());
            view.read_with(cx, |this, _| assert!(!this.error, "{}", this.status));
        }

        #[gpui::test]
        fn performance_bar_logs_actions_once_and_copies_bounded_history(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            for name in ["A", "B"] {
                std::fs::write(folder.path().join(format!("{name}.md")), name).unwrap();
            }
            let (view, cx) =
                cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
            let latest = |cx: &gpui::VisualTestContext| {
                view.read_with(cx, |this, cx| {
                    let entry = this.performance.read(cx).history.back().unwrap();
                    (entry.action, entry.target.clone())
                })
            };
            assert_eq!(latest(cx), ("Open note", "A.md".into()));
            view.update_in(cx, |this, w, cx| this.open_note("B.md".into(), w, cx));
            cx.run_until_parked();
            assert_eq!(latest(cx), ("Open note", "B.md".into()));
            let before = view.read_with(cx, |this, cx| this.performance.read(cx).history.len());
            click(cx, "tab-0");
            cx.run_until_parked();
            assert_eq!(latest(cx), ("Switch tab", "A.md".into()));
            view.read_with(cx, |this, cx| {
                assert_eq!(this.performance.read(cx).history.len(), before + 1)
            });
            cx.simulate_input("mouse ");
            view.read_with(cx, |this, cx| {
                assert_eq!(this.tabs[0].editor.read(cx).buffer.text(), "mouse A")
            });
            cx.simulate_keystrokes("ctrl-tab");
            assert_eq!(latest(cx), ("Switch tab", "B.md".into()));
            cx.simulate_input("edited ");
            cx.simulate_keystrokes("ctrl-w");
            assert_eq!(latest(cx), ("Close tab", "B.md".into()));
            view.read_with(cx, |this, cx| {
                assert!(
                    this.performance
                        .read(cx)
                        .history
                        .iter()
                        .any(|e| e.action == "Save note")
                )
            });
            for _ in 0..25 {
                view.update_in(cx, |this, w, cx| this.open_note("A.md".into(), w, cx));
                cx.run_until_parked();
            }
            assert_eq!(latest(cx), ("Tab already open", "A.md".into()));
            view.read_with(cx, |this, cx| {
                assert_eq!(this.performance.read(cx).history.len(), 20)
            });
            click(cx, "performance-log");
            view.read_with(cx, |this, cx| {
                assert_eq!(
                    cx.read_from_clipboard().unwrap().text().unwrap(),
                    this.performance.read(cx).log_text()
                );
            });
            // The measurement spans through paint, not just the synchronous handler.
            view.update(cx, |this, cx| {
                this.log_performance(
                    "Test",
                    "paint boundary".into(),
                    std::time::Instant::now() - std::time::Duration::from_millis(50),
                    cx,
                );
                assert_ne!(
                    this.performance.read(cx).history.back().unwrap().action,
                    "Test"
                );
            });
            cx.run_until_parked();
            view.read_with(cx, |this, cx| {
                let entry = this.performance.read(cx).history.back().unwrap();
                assert_eq!(entry.action, "Test");
                assert!(entry.elapsed >= std::time::Duration::from_millis(50));
            });
            // A conflicting close is measured as blocked, never as successful.
            cx.simulate_input("local ");
            std::fs::write(folder.path().join("A.md"), "external").unwrap();
            cx.simulate_keystrokes("ctrl-w");
            assert_eq!(latest(cx), ("Close blocked", "A.md".into()));
            view.read_with(cx, |this, _| assert_eq!(this.tabs.len(), 1));
        }

        #[gpui::test]
        fn tab_press_activates_and_repeated_open_reuses_views(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(
                folder.path().join("A.md"),
                (0..150).map(|i| format!("#tag{i} ")).collect::<String>(),
            )
            .unwrap();
            std::fs::write(folder.path().join("B.md"), "[[A]]").unwrap();
            let (view, cx) =
                cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
            view.update_in(cx, |this, w, cx| this.open_note("B.md".into(), w, cx));
            cx.run_until_parked();
            let point = cx.debug_bounds("tab-0").unwrap().center();
            cx.simulate_mouse_down(point, gpui::MouseButton::Left, gpui::Modifiers::default());
            view.read_with(cx, |this, _| assert_eq!(this.active, 0));
            cx.simulate_mouse_up(point, gpui::MouseButton::Left, gpui::Modifiers::default());
            cx.run_until_parked();
            view.update(cx, |this, cx| {
                this.connections.update(cx, |panel, cx| {
                    panel.scroll.scroll_to_item(40, gpui::ScrollStrategy::Top);
                    cx.notify();
                });
            });
            cx.run_until_parked();
            let scroll_top = view.read_with(cx, |this, cx| {
                this.connections
                    .read(cx)
                    .scroll
                    .0
                    .borrow()
                    .base_handle
                    .offset()
            });
            assert!(scroll_top.y < gpui::px(0.));
            let (editor, items, counts) = view.read_with(cx, |this, cx| {
                (
                    this.tabs[0].editor.entity_id(),
                    this.connections.read(cx).items.clone(),
                    (
                        this.tabs[0].editor.read(cx).render_count,
                        this.connections.read(cx).render_count,
                        this.sidebar.read(cx).render_count,
                    ),
                )
            });
            view.update_in(cx, |this, w, cx| this.open_note("A.md".into(), w, cx));
            cx.run_until_parked();
            view.read_with(cx, |this, cx| {
                assert_eq!(this.tabs.len(), 2);
                assert_eq!(this.tabs[0].editor.entity_id(), editor);
                assert_eq!(
                    (
                        this.tabs[0].editor.read(cx).render_count,
                        this.connections.read(cx).render_count,
                        this.sidebar.read(cx).render_count
                    ),
                    counts
                );
            });
            cx.simulate_keystrokes("ctrl-tab ctrl-shift-tab");
            view.read_with(cx, |this, cx| {
                assert_eq!(this.active, 0);
                assert!(std::sync::Arc::ptr_eq(
                    &items,
                    &this.connections.read(cx).items
                ));
                assert_eq!(
                    this.connections
                        .read(cx)
                        .scroll
                        .0
                        .borrow()
                        .base_handle
                        .offset(),
                    scroll_top
                );
            });
            // Saving a different note invalidates the cached incoming links.
            view.update_in(cx, |this, w, cx| this.activate_tab(1, w, cx));
            cx.simulate_keystrokes("ctrl-a");
            cx.simulate_input("no links");
            cx.simulate_keystrokes("ctrl-s ctrl-shift-tab");
            view.read_with(cx, |this, cx| {
                assert_eq!(this.connections.read(cx).backlinks, 0);
                assert!(!std::sync::Arc::ptr_eq(
                    &items,
                    &this.connections.read(cx).items
                ));
            });
        }

        #[gpui::test]
        fn tab_drag_close_and_keyboard_preserve_active_document(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            for name in ["A", "B", "C"] {
                std::fs::write(folder.path().join(format!("{name}.md")), name).unwrap();
            }
            let (view, cx) =
                cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
            for name in ["B.md", "C.md"] {
                view.update_in(cx, |this, w, cx| this.open_note(name.into(), w, cx));
                cx.run_until_parked();
            }
            drag(cx, "tab-0", "tabs-end");
            view.read_with(cx, |this, _| {
                assert_eq!(
                    this.tabs.iter().map(|t| t.path.clone()).collect::<Vec<_>>(),
                    ["B.md", "C.md", "A.md"].map(PathBuf::from)
                );
                assert_eq!(this.tabs[this.active].path, PathBuf::from("A.md"));
            });
            cx.simulate_input("edited ");
            cx.simulate_keystrokes("ctrl-shift-tab");
            click(cx, "close-tab-2"); // Close a dirty background tab, retaining C.
            assert_eq!(
                std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
                "edited A"
            );
            view.read_with(cx, |this, _| {
                assert_eq!(this.tabs.len(), 2);
                assert_eq!(this.tabs[this.active].path, PathBuf::from("C.md"));
            });
            cx.simulate_keystrokes("ctrl-1");
            view.read_with(cx, |this, _| {
                assert_eq!(this.tabs[this.active].path, PathBuf::from("B.md"))
            });
            cx.simulate_keystrokes("ctrl-9");
            let point = cx.debug_bounds("tab-0").unwrap().center();
            cx.simulate_mouse_down(point, gpui::MouseButton::Middle, gpui::Modifiers::default());
            cx.simulate_mouse_up(point, gpui::MouseButton::Middle, gpui::Modifiers::default());
            view.read_with(cx, |this, _| assert_eq!(this.tabs.len(), 1));
            click(cx, "close-tab-0");
            view.read_with(cx, |this, _| assert!(this.tabs.is_empty()));
        }

        #[gpui::test]
        fn mouse_moves_and_background_tab_close_preserve_conflicting_edits(
            cx: &mut TestAppContext,
        ) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::create_dir(folder.path().join("Archive")).unwrap();
            std::fs::write(folder.path().join("A.md"), "original").unwrap();
            std::fs::write(folder.path().join("B.md"), "other").unwrap();
            let (view, cx) =
                cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
            cx.simulate_input("local ");
            std::fs::write(folder.path().join("A.md"), "external").unwrap();
            view.update_in(cx, |this, w, cx| this.open_note("B.md".into(), w, cx));
            cx.run_until_parked();
            click(cx, "close-tab-0");
            view.read_with(cx, |this, cx| {
                assert_eq!(this.tabs.len(), 2);
                assert_eq!(this.tabs[this.active].path, PathBuf::from("A.md"));
                assert_eq!(
                    this.tabs[this.active].editor.read(cx).buffer.text(),
                    "local original"
                );
                assert!(this.error);
            });
            drag(cx, "entry:A.md", "entry:Archive");
            assert!(!folder.path().join("Archive/A.md").exists());
            assert_eq!(
                std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
                "external"
            );
            view.read_with(cx, |this, cx| {
                assert!(!this.managing);
                assert_eq!(
                    this.tabs[this.active].editor.read(cx).buffer.text(),
                    "local original"
                );
            });
        }

        #[gpui::test]
        fn explorer_menu_keyboard_dismissal_and_name_collision(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("A.md"), "a").unwrap();
            std::fs::write(folder.path().join("B.md"), "b").unwrap();
            let (view, cx) =
                cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
            right_click(cx, "entry:B.md");
            click(cx, "explorer-action-0");
            cx.simulate_input("A");
            cx.simulate_keystrokes("enter");
            view.read_with(cx, |this, _| {
                assert!(this.error);
                assert!(this.inline_parent.is_some());
            });
            assert_eq!(
                std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
                "a"
            );
            assert_eq!(
                std::fs::read_to_string(folder.path().join("B.md")).unwrap(),
                "b"
            );
            cx.simulate_keystrokes("escape shift-f10");
            view.read_with(cx, |this, _| assert!(this.explorer_menu.is_some()));
            click(cx, "tab-0");
            view.read_with(cx, |this, _| assert!(this.explorer_menu.is_none()));
            // A shortcut displayed in the menu also works while it is open.
            right_click(cx, "vault-location");
            cx.simulate_keystrokes("ctrl-n");
            cx.simulate_input("From shortcut");
            cx.simulate_keystrokes("enter");
            assert!(folder.path().join("From shortcut.md").exists());
        }

        #[gpui::test]
        fn keyboard_tab_switching_reveals_tabs_in_narrow_window(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            for i in 0..12 {
                std::fs::write(folder.path().join(format!("Note{i:02}.md")), "text").unwrap();
            }
            let (view, cx) =
                cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
            cx.simulate_resize(gpui::size(gpui::px(880.), gpui::px(560.)));
            for i in 1..12 {
                view.update_in(cx, |this, w, cx| {
                    this.open_note(format!("Note{i:02}.md").into(), w, cx)
                });
                cx.run_until_parked();
            }
            cx.simulate_keystrokes("ctrl-1");
            let first = cx.debug_bounds("tab-0").unwrap();
            assert!(first.left() >= gpui::px(270.) && first.right() <= gpui::px(880.));
            cx.simulate_keystrokes("ctrl-9");
            let last = cx.debug_bounds("tab-11").unwrap();
            assert!(last.left() >= gpui::px(270.) && last.right() <= gpui::px(880.));
            view.read_with(cx, |this, _| assert_eq!(this.active, 11));
        }

        #[gpui::test]
        fn reading_column_and_complete_code_clipboard(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            let code = "let greeting = \"你好\";\n".repeat(100);
            std::fs::write(folder.path().join("A.md"), format!("```rust\n{code}```")).unwrap();
            std::fs::write(folder.path().join("Empty.md"), "").unwrap();
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            view.update_in(cx, |this, window, cx| {
                this.set_mode(super::ViewMode::Preview, window, cx)
            });
            cx.simulate_resize(gpui::size(gpui::px(1600.), gpui::px(900.)));
            cx.run_until_parked();
            let wide = cx.debug_bounds("preview-column-0").unwrap();
            assert_eq!(wide.size.width, gpui::px(760.));
            let copy = cx.debug_bounds("copy-code-0").unwrap();
            cx.simulate_click(copy.center(), gpui::Modifiers::default());
            view.read_with(cx, |this, cx| {
                assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), code);
                assert_eq!(this.status, "Code copied to clipboard");
            });
            cx.simulate_resize(gpui::size(gpui::px(880.), gpui::px(560.)));
            cx.run_until_parked();
            let narrow = cx.debug_bounds("preview-column-0").unwrap();
            assert!(narrow.size.width < wide.size.width);
            assert!(narrow.right() <= gpui::px(880.));
            view.update_in(cx, |this, window, cx| {
                this.open_note("Empty.md".into(), window, cx)
            });
            cx.run_until_parked();
            // GPUI's test-only selector map isn't carried over by cached paint.
            // Locate the button in a fresh draw, then exercise its cached handler.
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
            let start = cx.debug_bounds("empty-source").unwrap();
            view.update(cx, |_, cx| cx.notify());
            cx.run_until_parked();
            cx.simulate_click(start.center(), gpui::Modifiers::default());
            view.read_with(cx, |this, _| assert!(this.mode == super::ViewMode::Source));
            cx.simulate_input("New thought");
            view.read_with(cx, |this, cx| {
                assert_eq!(
                    this.tabs[this.active].editor.read(cx).buffer.text(),
                    "New thought"
                )
            });
        }

        #[gpui::test]
        fn tree_keyboard_reveal_and_search_restore_exact_scroll(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(folder.path().join("Zebra/Empty")).unwrap();
            for i in 0..400 {
                std::fs::write(folder.path().join(format!("Zebra/Note{i:04}.md")), "body").unwrap();
            }
            std::fs::write(folder.path().join("Welcome.md"), "welcome").unwrap();
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            view.read_with(cx, |this, _| {
                assert_eq!(this.tree.visible.len(), 2);
                assert_eq!(this.tree.visible[0].path, PathBuf::from("Zebra"));
                assert!(this.navigation.expanded.is_empty());
            });
            view.update_in(cx, |this, window, _| this.tree_focus.focus(window));
            cx.simulate_keystrokes("down");
            view.read_with(cx, |this, _| {
                assert_eq!(this.navigation.selected, Some("Zebra".into()))
            });
            cx.simulate_keystrokes("right right");
            view.read_with(cx, |this, _| {
                assert_eq!(this.navigation.selected, Some("Zebra/Empty".into()))
            });
            cx.simulate_keystrokes("left left");
            view.read_with(cx, |this, _| assert_eq!(this.tree.visible.len(), 2));
            cx.simulate_keystrokes("w enter");
            view.read_with(cx, |this, _| {
                assert_eq!(this.tabs[this.active].path, PathBuf::from("Welcome.md"))
            });
            view.update_in(cx, |this, w, cx| {
                this.open_note("Zebra/Note0399.md".into(), w, cx)
            });
            cx.run_until_parked();
            view.read_with(cx, |this, _| {
                assert!(
                    this.navigation.expanded.is_empty(),
                    "opening a note must not expand the tree"
                )
            });
            cx.simulate_keystrokes("ctrl-shift-b");
            let offset = view.read_with(cx, |this, _| {
                assert!(this.navigation.expanded.contains(&PathBuf::from("Zebra")));
                assert_eq!(this.navigation.selected, Some("Zebra/Note0399.md".into()));
                let offset = this.notes_scroll.0.borrow().base_handle.offset();
                assert!(offset.y < gpui::px(-100.));
                offset
            });
            cx.simulate_keystrokes("ctrl-f");
            cx.simulate_input("Note0001");
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(150));
            cx.run_until_parked();
            view.read_with(cx, |this, _| assert_eq!(this.search_results.len(), 1));
            cx.simulate_keystrokes("ctrl-a backspace");
            view.read_with(cx, |this, _| {
                assert!(
                    (this.notes_scroll.0.borrow().base_handle.offset().y - offset.y).abs()
                        < gpui::px(0.01)
                );
                assert!(this.navigation.expanded.contains(&PathBuf::from("Zebra")));
            });
        }

        #[gpui::test]
        fn sidebar_resize_hide_and_per_vault_restore(cx: &mut TestAppContext) {
            let first = tempfile::tempdir().unwrap();
            let second = tempfile::tempdir().unwrap();
            let state = tempfile::tempdir().unwrap();
            std::fs::create_dir(first.path().join("Folder")).unwrap();
            for i in 0..100 {
                std::fs::write(first.path().join(format!("Folder/Note{i:03}.md")), "").unwrap();
            }
            let (view, cx) = cx.add_window_view(|window, cx| {
                let mut view = Workspace::new(None, window, cx);
                view.state_directory = Some(state.path().into());
                view.open_vault(first.path().into(), window, cx);
                view
            });
            cx.simulate_mouse_down(
                gpui::point(gpui::px(272.), gpui::px(300.)),
                gpui::MouseButton::Left,
                gpui::Modifiers::default(),
            );
            cx.simulate_mouse_move(
                gpui::point(gpui::px(355.), gpui::px(300.)),
                gpui::MouseButton::Left,
                gpui::Modifiers::default(),
            );
            cx.simulate_mouse_up(
                gpui::point(gpui::px(355.), gpui::px(300.)),
                gpui::MouseButton::Left,
                gpui::Modifiers::default(),
            );
            view.read_with(cx, |this, _| assert_eq!(this.navigation.width, 355.));
            view.update(cx, |this, cx| {
                this.toggle_folder("Folder".into(), cx);
                this.notes_scroll
                    .scroll_to_item(80, gpui::ScrollStrategy::Top);
            });
            cx.run_until_parked();
            let offset = view.read_with(cx, |this, _| {
                this.notes_scroll.0.borrow().base_handle.offset()
            });
            cx.simulate_keystrokes("ctrl-b");
            view.read_with(cx, |this, _| assert!(this.navigation.hidden));
            view.update_in(cx, |this, w, cx| {
                this.open_vault(second.path().into(), w, cx)
            });
            cx.run_until_parked();
            view.read_with(cx, |this, _| {
                assert_eq!(this.navigation.width, 270.);
                assert!(!this.navigation.hidden);
                assert!(this.navigation.expanded.is_empty());
            });
            view.update_in(cx, |this, w, cx| {
                this.open_vault(first.path().into(), w, cx)
            });
            cx.run_until_parked();
            view.read_with(cx, |this, _| {
                assert_eq!(this.navigation.width, 355.);
                assert!(this.navigation.hidden);
                assert!(this.navigation.expanded.contains(&PathBuf::from("Folder")));
            });
            cx.simulate_keystrokes("ctrl-b");
            view.read_with(cx, |this, _| {
                let restored = this.notes_scroll.0.borrow().base_handle.offset();
                assert_eq!(restored.x, offset.x);
                // Flex layout and serialized f32 scroll offsets can differ below a pixel.
                assert!(f32::from(restored.y - offset.y).abs() < 0.01);
            });
        }

        #[gpui::test]
        fn folder_create_rename_move_updates_open_tabs_and_wiki_links(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::create_dir(folder.path().join("Ideas")).unwrap();
            std::fs::create_dir(folder.path().join("Archive")).unwrap();
            std::fs::write(folder.path().join("Welcome.md"), "[[Ideas/Note|Link]]").unwrap();
            std::fs::write(folder.path().join("Ideas/Note.md"), "# Note\r\nbody\r\n").unwrap();
            let (view, cx) =
                cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
            view.update_in(cx, |this, w, cx| {
                this.navigation.selected = Some("Ideas".into());
                this.show_modal(super::Modal::NewFolder, w, cx);
            });
            cx.simulate_input("Empty");
            cx.simulate_keystrokes("enter");
            assert!(folder.path().join("Ideas/Empty").is_dir());
            cx.simulate_keystrokes("ctrl-n");
            cx.simulate_input("Fresh");
            cx.simulate_keystrokes("enter");
            assert!(folder.path().join("Ideas/Empty/Fresh.md").is_file());
            view.update_in(cx, |this, w, cx| {
                this.open_note("Ideas/Note.md".into(), w, cx)
            });
            cx.run_until_parked();
            cx.simulate_keystrokes("ctrl-end");
            cx.simulate_input("edited");
            view.update_in(cx, |this, w, cx| {
                this.navigation.selected = Some("Ideas".into());
                this.show_modal(super::Modal::Rename, w, cx);
            });
            cx.simulate_input("Thoughts");
            cx.simulate_keystrokes("enter");
            view.read_with(cx, |this, cx| {
                assert!(!this.managing);
                assert!(!this.error, "{}", this.status);
                assert_eq!(
                    this.tabs[this.active].path,
                    PathBuf::from("Thoughts/Note.md")
                );
                assert!(!this.tabs[this.active].dirty(cx));
                let welcome = this
                    .tabs
                    .iter()
                    .find(|t| t.path == PathBuf::from("Welcome.md"))
                    .unwrap();
                assert_eq!(
                    welcome.editor.read(cx).buffer.text(),
                    "[[Thoughts/Note|Link]]"
                );
                assert!(!welcome.dirty(cx));
            });
            assert_eq!(
                std::fs::read_to_string(folder.path().join("Thoughts/Note.md")).unwrap(),
                "# Note\r\nbody\r\nedited"
            );
            view.update_in(cx, |this, w, cx| {
                this.navigation.selected = Some("Thoughts/Note.md".into());
                this.show_modal(super::Modal::Move, w, cx);
            });
            cx.simulate_input("Archive");
            cx.simulate_keystrokes("enter");
            view.read_with(cx, |this, cx| {
                assert!(!this.error, "{}", this.status);
                assert_eq!(
                    this.tabs[this.active].path,
                    PathBuf::from("Archive/Note.md")
                );
                assert!(!this.tabs[this.active].dirty(cx));
                assert_eq!(
                    this.vault
                        .as_ref()
                        .unwrap()
                        .backlinks(std::path::Path::new("Archive/Note.md"))
                        .len(),
                    1
                );
            });
            assert!(folder.path().join("Thoughts/Empty/Fresh.md").is_file());
            assert_eq!(
                std::fs::read_to_string(folder.path().join("Welcome.md")).unwrap(),
                "[[Archive/Note|Link]]"
            );
        }

        #[gpui::test]
        fn move_keeps_window_open_and_blocks_document_edits_until_complete(
            cx: &mut TestAppContext,
        ) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("A.md"), "original").unwrap();
            let (view, cx) =
                cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
            view.update_in(cx, |this, w, cx| {
                this.navigation.selected = Some("A.md".into());
                this.show_modal(super::Modal::Rename, w, cx);
                this.move_entry("A.md".into(), "B.md".into(), w, cx);
                assert!(this.managing);
                assert!(this.focus.is_focused(w));
                assert!(!this.tabs[0].editor.read(cx).focus.is_focused(w));
                assert!(!this.save_all_in(w, cx));
                this.dismiss(w, cx);
                assert!(this.modal == super::Modal::Rename);
            });
            cx.run_until_parked();
            view.read_with(cx, |this, cx| {
                assert!(!this.managing);
                assert_eq!(this.tabs[0].path, PathBuf::from("B.md"));
                assert_eq!(this.tabs[0].editor.read(cx).buffer.text(), "original");
                assert!(!this.error, "{}", this.status);
            });
        }

        #[gpui::test]
        fn move_stops_on_dirty_tab_conflict(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("A.md"), "original").unwrap();
            let (view, cx) =
                cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
            cx.simulate_input("dirty");
            std::fs::write(folder.path().join("A.md"), "external").unwrap();
            view.update_in(cx, |this, w, cx| {
                this.navigation.selected = Some("A.md".into());
                this.show_modal(super::Modal::Rename, w, cx);
            });
            cx.simulate_input("B");
            cx.simulate_keystrokes("enter");
            view.read_with(cx, |this, cx| {
                assert!(this.error);
                assert!(!this.managing);
                assert!(this.tabs[0].dirty(cx));
                assert_eq!(this.tabs[0].path, PathBuf::from("A.md"));
            });
            assert!(!folder.path().join("B.md").exists());
            assert_eq!(
                std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
                "external"
            );
        }

        #[gpui::test]
        fn disk_reload_preserves_edits_made_after_the_request(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("A.md"), "original").unwrap();
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            std::fs::write(folder.path().join("A.md"), "external").unwrap();
            view.update(cx, |this, cx| {
                this.reload_tab(0, cx);
                this.tabs[0]
                    .editor
                    .update(cx, |editor, cx| editor.set_text("new local edit", cx));
            });
            cx.run_until_parked();
            view.read_with(cx, |this, cx| {
                assert!(this.error);
                assert!(this.tabs[0].dirty(cx));
                assert_eq!(this.tabs[0].baseline, "original");
                assert_eq!(this.tabs[0].editor.read(cx).buffer.text(), "new local edit");
            });
            view.update(cx, |this, cx| this.reload_tab(0, cx));
            cx.run_until_parked();
            view.read_with(cx, |this, cx| {
                assert!(!this.tabs[0].dirty(cx));
                assert_eq!(this.tabs[0].editor.read(cx).buffer.text(), "external");
            });
        }

        #[gpui::test]
        fn pending_note_loads_do_not_override_newer_navigation(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            for name in ["A", "B", "C"] {
                std::fs::write(folder.path().join(format!("{name}.md")), name).unwrap();
            }
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            view.update_in(cx, |this, window, cx| {
                this.open_note("B.md".into(), window, cx);
                assert_eq!(this.tabs[this.active].path, PathBuf::from("A.md"));
                this.open_note("C.md".into(), window, cx);
            });
            cx.run_until_parked();
            view.read_with(cx, |this, _| {
                assert_eq!(this.tabs[this.active].path, PathBuf::from("C.md"))
            });
            view.update_in(cx, |this, window, cx| {
                this.open_note("B.md".into(), window, cx);
                this.open_note("A.md".into(), window, cx);
            });
            cx.run_until_parked();
            view.read_with(cx, |this, _| {
                assert_eq!(this.tabs[this.active].path, PathBuf::from("A.md"))
            });
        }

        #[gpui::test]
        fn preview_and_dirty_state_track_edits_undo_and_async_heading_navigation(
            cx: &mut TestAppContext,
        ) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("A.md"), "# First\n\n#old").unwrap();
            std::fs::write(
                folder.path().join("B.md"),
                format!("{}\n# Destination\n", "paragraph\n\n".repeat(4000)),
            )
            .unwrap();
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            cx.simulate_keystrokes("ctrl-a");
            cx.simulate_input("#new");
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(75));
            cx.run_until_parked();
            view.read_with(cx, |this, cx| {
                assert_eq!(this.tabs[0].prepared.tags, vec!["new"]);
                assert!(this.tabs[0].dirty(cx));
            });
            cx.simulate_keystrokes("ctrl-z ctrl-z ctrl-z ctrl-z");
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(75));
            cx.run_until_parked();
            view.read_with(cx, |this, cx| assert!(!this.tabs[0].dirty(cx)));
            view.update_in(cx, |this, window, cx| {
                this.open_note_at("B.md".into(), Some("Destination".into()), window, cx)
            });
            cx.run_until_parked();
            view.read_with(cx, |this, cx| {
                let tab = &this.tabs[this.active];
                let editor = tab.editor.read(cx);
                assert_eq!(
                    &editor.buffer.text()[editor.buffer.cursor()..],
                    "# Destination\n"
                );
                assert!(
                    tab.preview_scroll
                        .bounds_for_item(tab.prepared.headings[0].1)
                        .is_some()
                );
            });
            view.update(cx, |this, cx| {
                this.tabs[this.active]
                    .preview_scroll
                    .scroll_to(gpui::ListOffset {
                        item_ix: 0,
                        offset_in_item: gpui::px(0.),
                    });
                this.preview_pane.update(cx, |_, cx| cx.notify());
            });
            cx.run_until_parked();
            view.update_in(cx, |this, window, cx| {
                this.open_note_at("B.md".into(), Some("Destination".into()), window, cx);
            });
            cx.run_until_parked();
            view.read_with(cx, |this, _| {
                let tab = &this.tabs[this.active];
                assert!(
                    tab.preview_scroll
                        .bounds_for_item(tab.prepared.headings[0].1)
                        .is_some()
                );
            });
        }

        #[gpui::test]
        fn scrolling_only_renders_the_changed_pane_for_short_and_long_notes(
            cx: &mut TestAppContext,
        ) {
            let folder = tempfile::tempdir().unwrap();
            for (name, lines) in [("A", 50), ("B", 20_000)] {
                std::fs::write(
                    folder.path().join(format!("{name}.md")),
                    "A paragraph with [[Missing]].\n\n".repeat(lines),
                )
                .unwrap();
            }
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            for name in ["A.md", "B.md"] {
                view.update_in(cx, |this, window, cx| {
                    this.open_note(name.into(), window, cx)
                });
                cx.run_until_parked();
                let counts = |cx: &gpui::VisualTestContext| {
                    view.read_with(cx, |this, cx| {
                        (
                            this.tabs[this.active].editor.read(cx).render_count,
                            this.preview_pane.read(cx).render_count,
                            this.sidebar.read(cx).render_count,
                            this.connections.read(cx).render_count,
                        )
                    })
                };
                for pane in ["source-pane", "preview-pane"] {
                    let before = counts(cx);
                    let position = cx.debug_bounds(pane).unwrap().center();
                    for _ in 0..5 {
                        cx.simulate_event(gpui::ScrollWheelEvent {
                            position,
                            delta: gpui::ScrollDelta::Pixels(gpui::point(
                                gpui::px(0.),
                                gpui::px(-60.),
                            )),
                            ..Default::default()
                        });
                        cx.run_until_parked();
                    }
                    let after = counts(cx);
                    if pane == "source-pane" {
                        assert!(after.0 > before.0);
                        assert_eq!(after.1, before.1, "source scroll redrew preview");
                    } else {
                        assert_eq!(after.0, before.0, "preview scroll redrew editor");
                        assert!(after.1 > before.1);
                        view.read_with(cx, |this, _| {
                            assert!(
                                this.tabs[this.active]
                                    .preview_scroll
                                    .logical_scroll_top()
                                    .item_ix
                                    > 0
                            )
                        });
                    }
                    assert_eq!(after.2, before.2, "scroll redrew explorer");
                    assert_eq!(after.3, before.3, "scroll redrew connections");
                    view.read_with(cx, |this, cx| {
                        assert_eq!(this.tabs[this.active].editor.read(cx).buffer.cursor(), 0);
                        assert!(!this.tabs[this.active].dirty(cx));
                    });
                }
            }
        }

        #[gpui::test]
        fn connections_are_virtualized_and_last_backlink_and_tag_are_clickable(
            cx: &mut TestAppContext,
        ) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("A.md"), "#topic").unwrap();
            for i in 0..400 {
                std::fs::write(folder.path().join(format!("Incoming{i:04}.md")), "[[A]]").unwrap();
            }
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            view.read_with(cx, |this, cx| {
                let panel = this.connections.read(cx);
                assert_eq!(panel.backlinks, 400);
                assert_eq!(panel.tags, 1);
            });
            assert!(cx.debug_bounds("connection-0").is_some());
            assert!(cx.debug_bounds("connection-399").is_none());
            view.update(cx, |this, cx| {
                this.connections.update(cx, |panel, cx| {
                    panel.scroll.scroll_to_item(
                        panel.items.len().div_ceil(3) - 1,
                        gpui::ScrollStrategy::Bottom,
                    );
                    cx.notify();
                });
            });
            cx.run_until_parked();
            click(cx, "connection-400");
            view.read_with(cx, |this, cx| {
                assert_eq!(this.search.read(cx).buffer.text(), "#topic")
            });
            click(cx, "connection-399");
            view.read_with(cx, |this, cx| {
                assert_eq!(
                    this.tabs[this.active].path,
                    PathBuf::from("Incoming0399.md")
                );
                assert_eq!(this.connections.read(cx).backlinks, 0);
            });
        }

        #[gpui::test]
        fn first_opening_frame_precedes_full_analysis_and_preserves_immediate_edits(
            cx: &mut TestAppContext,
        ) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("A.md"), "start").unwrap();
            let original = format!(
                "# First\n\n{}\n# Last\n\n[[Missing]]",
                "paragraph\n\n".repeat(4000)
            );
            std::fs::write(folder.path().join("B.md"), &original).unwrap();
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            let initial_seen = std::rc::Rc::new(std::cell::Cell::new(false));
            let completed_seen = std::rc::Rc::new(std::cell::Cell::new(false));
            let initial = initial_seen.clone();
            let completed = completed_seen.clone();
            let expected = original.clone();
            view.update_in(cx, |this, window, cx| {
                this._subscriptions
                    .push(cx.observe(&cx.entity(), move |this, _, cx| {
                        let Some(tab) = this
                            .tabs
                            .get(this.active)
                            .filter(|t| t.path == PathBuf::from("B.md"))
                        else {
                            return;
                        };
                        if !tab.prepared.complete && !initial.replace(true) {
                            assert!(tab.initial_frame_pending);
                            assert!(tab.editor.read(cx).first_paint_at.is_none());
                            assert_eq!(tab.editor.read(cx).buffer.text(), expected);
                            assert_eq!(tab.prepared.rows[0].text.as_ref(), "First");
                            assert!(tab.prepared.rows.len() < 1000);
                            // Type while only the opening preview is available.
                            tab.editor.update(cx, |editor, cx| {
                                editor.buffer.move_to(editor.buffer.text().len(), false);
                                editor.buffer.insert("\n\n#typed");
                                cx.notify();
                            });
                        } else if tab.prepared.complete {
                            assert!(tab.editor.read(cx).first_paint_at.is_some());
                            completed.set(true);
                        }
                    }));
                this.open_note("B.md".into(), window, cx);
            });
            cx.run_until_parked();
            assert!(initial_seen.get());
            assert!(completed_seen.get());
            view.read_with(cx, |this, cx| {
                let tab = &this.tabs[this.active];
                assert_eq!(
                    tab.editor.read(cx).buffer.text(),
                    format!("{original}\n\n#typed")
                );
                assert_eq!(tab.baseline, original);
                assert!(tab.dirty(cx));
                assert_eq!(tab.prepared_revision, tab.editor.read(cx).buffer.revision());
                assert_eq!(tab.prepared.tags, vec!["typed"]);
                assert!(!tab.editor.read(cx).missing_links.is_empty());
            });
        }

        #[gpui::test]
        fn incomplete_code_preview_never_copies_a_truncated_block(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            let code = "let greeting = \"你好\";\n".repeat(2000);
            let source = format!("```rust\n{code}```");
            std::fs::write(folder.path().join("A.md"), &source).unwrap();
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            // Hold the opening stage to exercise its controls before completion.
            view.update(cx, |this, cx| {
                let tab = &mut this.tabs[this.active];
                tab.prepared = std::sync::Arc::new(opalglass::prepared::Prepared::opening(&source));
                tab.initial_frame_pending = false;
                let count = tab.prepared.rows.len();
                tab.preview_scroll.reset(count);
                cx.notify();
            });
            cx.run_until_parked();
            click(cx, "copy-code-0");
            view.read_with(cx, |this, cx| {
                assert_eq!(this.status, "This code block is still loading.");
                assert!(cx.read_from_clipboard().is_none());
            });
            view.update(cx, |this, cx| {
                this.tabs[this.active].requested_revision = u64::MAX;
                this.update_document(cx);
            });
            cx.run_until_parked();
            click(cx, "copy-code-0");
            view.read_with(cx, |this, cx| {
                assert!(this.tabs[this.active].prepared.complete);
                assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), code);
            });
        }

        #[gpui::test]
        #[ignore]
        fn profile_tab_work(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            let text = std::env::var_os("OPALGLASS_PROFILE_NOTE")
                .map(|p| std::fs::read_to_string(p).unwrap())
                .unwrap_or_else(|| "paragraph with [[A]] and #tag.\n\n".repeat(8000));
            for name in ["A", "B", "C"] {
                std::fs::write(folder.path().join(format!("{name}.md")), &text).unwrap();
            }
            let incoming = std::env::var("OPALGLASS_PROFILE_BACKLINKS")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            for i in 0..incoming {
                std::fs::write(
                    folder.path().join(format!("Incoming{i:04}.md")),
                    "[[A]] [[B]] [[C]]",
                )
                .unwrap();
            }
            let (view, cx) =
                cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
            for path in ["B.md", "C.md"] {
                view.update_in(cx, |this, w, cx| this.open_note(path.into(), w, cx));
                cx.run_until_parked();
                cx.simulate_keystrokes("ctrl-end");
            }
            for mode in [
                super::ViewMode::Source,
                super::ViewMode::Split,
                super::ViewMode::Preview,
            ] {
                view.update_in(cx, |this, w, cx| this.set_mode(mode, w, cx));
                cx.run_until_parked();
                let start = std::time::Instant::now();
                for i in 0..30 {
                    view.update_in(cx, |this, w, cx| this.activate_tab(i % 3, w, cx));
                    cx.run_until_parked();
                }
                println!(
                    "30 existing tab switches, mode {}: {:?}",
                    mode as u8,
                    start.elapsed()
                );
                let start = std::time::Instant::now();
                for _ in 0..10 {
                    view.update_in(cx, |this, w, cx| this.open_note("C.md".into(), w, cx));
                    cx.run_until_parked();
                }
                println!(
                    "10 already-active opens, mode {}: {:?}",
                    mode as u8,
                    start.elapsed()
                );
            }
            let start = std::time::Instant::now();
            view.update_in(cx, |this, w, cx| this.close_tab_at(1, w, cx));
            cx.run_until_parked();
            println!("close clean background tab: {:?}", start.elapsed());
            view.update_in(cx, |this, w, cx| {
                this.set_mode(super::ViewMode::Source, w, cx)
            });
            cx.simulate_input("edited");
            let start = std::time::Instant::now();
            view.update_in(cx, |this, w, cx| this.close_tab(w, cx));
            cx.run_until_parked();
            println!("save and close active tab: {:?}", start.elapsed());
        }

        #[gpui::test]
        #[ignore]
        fn profile_note_work(cx: &mut TestAppContext) {
            let text = std::env::var_os("OPALGLASS_PROFILE_NOTE")
                .map(|path| std::fs::read_to_string(path).unwrap())
                .unwrap_or_else(|| {
                    "## Heading\n\nA paragraph with **bold** and [[Start]] and #tag.\n\n"
                        .repeat(4000)
                });
            let prep_start = std::time::Instant::now();
            let normalized = opalglass::document::normalize(&text);
            let buffer = opalglass::buffer::Buffer::new(normalized);
            println!("normalize + buffer: {:?}", prep_start.elapsed());
            let prep_start = std::time::Instant::now();
            let _rows = opalglass::document::source_rows(buffer.text());
            println!("source row indexing: {:?}", prep_start.elapsed());
            let prep_start = std::time::Instant::now();
            let _initial = opalglass::prepared::Prepared::opening(buffer.text());
            println!("opening preview: {:?}", prep_start.elapsed());
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("Start.md"), "start").unwrap();
            std::fs::write(folder.path().join("Target.md"), &text).unwrap();
            let incoming = std::env::var("OPALGLASS_PROFILE_BACKLINKS")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            for i in 0..incoming {
                std::fs::write(
                    folder.path().join(format!("Incoming{i:04}.md")),
                    "[[Target]]",
                )
                .unwrap();
            }
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            let start = std::time::Instant::now();
            view.update_in(cx, |this, window, cx| {
                this.open_note("Target.md".into(), window, cx)
            });
            cx.run_until_parked();
            let first_paint = view.read_with(cx, |this, cx| {
                assert!(this.tabs[this.active].prepared.complete);
                this.tabs[this.active]
                    .editor
                    .read(cx)
                    .first_paint_at
                    .unwrap()
                    .duration_since(start)
            });
            println!(
                "note bytes: {}; first text paint: {:?}; full preparation: {:?}",
                text.len(),
                first_paint,
                start.elapsed()
            );
            let start = std::time::Instant::now();
            for _ in 0..3 {
                view.update(cx, |_, cx| cx.notify());
                cx.run_until_parked();
            }
            println!("3 note redraws: {:?}", start.elapsed());
            for mode in [
                super::ViewMode::Source,
                super::ViewMode::Preview,
                super::ViewMode::Split,
            ] {
                view.update_in(cx, |this, window, cx| this.set_mode(mode, window, cx));
                cx.run_until_parked();
                for pane in ["source-pane", "preview-pane"] {
                    if (pane == "source-pane" && mode == super::ViewMode::Preview)
                        || (pane == "preview-pane" && mode == super::ViewMode::Source)
                    {
                        continue;
                    }
                    let Some(bounds) = cx.debug_bounds(pane) else {
                        continue;
                    };
                    let start = std::time::Instant::now();
                    for _ in 0..60 {
                        cx.simulate_event(gpui::ScrollWheelEvent {
                            position: bounds.center(),
                            delta: gpui::ScrollDelta::Pixels(gpui::point(
                                gpui::px(0.),
                                gpui::px(-40.),
                            )),
                            ..Default::default()
                        });
                        cx.run_until_parked();
                    }
                    println!(
                        "60 scroll frames (mode {}, {pane}): {:?}",
                        mode as u8,
                        start.elapsed()
                    );
                }
            }
            let start = std::time::Instant::now();
            cx.simulate_keystrokes("ctrl-end");
            cx.simulate_input("x");
            println!("jump to end + insert: {:?}", start.elapsed());
            let start = std::time::Instant::now();
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(75));
            cx.run_until_parked();
            println!("background preview update: {:?}", start.elapsed());
            let start = std::time::Instant::now();
            for path in ["Start.md", "Target.md", "Start.md", "Target.md"] {
                view.update_in(cx, |this, window, cx| {
                    this.open_note(path.into(), window, cx)
                });
                cx.run_until_parked();
            }
            println!("4 cached tab switches: {:?}", start.elapsed());
            let start = std::time::Instant::now();
            cx.simulate_keystrokes("ctrl-s");
            println!("save temporary copy: {:?}", start.elapsed());
        }

        #[gpui::test]
        fn virtual_note_list_opens_the_last_row_and_keeps_scroll_after_saving(
            cx: &mut TestAppContext,
        ) {
            let folder = tempfile::tempdir().unwrap();
            for index in 0..400 {
                std::fs::write(folder.path().join(format!("Note{index:04}.md")), "note").unwrap();
            }
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            view.update(cx, |this, cx| {
                this.notes_scroll
                    .scroll_to_item(399, gpui::ScrollStrategy::Bottom);
                cx.notify();
            });
            cx.run_until_parked();
            let (point, offset) = view.read_with(cx, |this, _| {
                let scroll = this.notes_scroll.0.borrow();
                let bounds = scroll.base_handle.bounds();
                (
                    gpui::point(
                        bounds.left() + gpui::px(60.),
                        bounds.bottom() - gpui::px(28.),
                    ),
                    scroll.base_handle.offset(),
                )
            });
            cx.simulate_click(point, gpui::Modifiers::default());
            view.read_with(cx, |this, _| {
                assert_eq!(this.tabs[this.active].path, PathBuf::from("Note0399.md"))
            });
            cx.simulate_keystrokes("ctrl-end");
            cx.simulate_input(" edited");
            cx.simulate_keystrokes("ctrl-s");
            view.read_with(cx, |this, _| {
                let after = this.notes_scroll.0.borrow().base_handle.offset();
                assert!((after.y - offset.y).abs() < gpui::px(0.01));
                assert_eq!(after.x, offset.x);
            });
            assert_eq!(
                std::fs::read_to_string(folder.path().join("Note0399.md")).unwrap(),
                "note edited"
            );
        }

        // Explicit opt-in: only opens and redraws the vault, never edits or saves.
        #[gpui::test]
        #[ignore]
        fn profile_vault_redraws(cx: &mut TestAppContext) {
            let root =
                std::env::var_os("OPALGLASS_PROFILE_VAULT").expect("set OPALGLASS_PROFILE_VAULT");
            let start = std::time::Instant::now();
            let milestones = std::rc::Rc::new(std::cell::RefCell::new([None; 3]));
            let recorded = milestones.clone();
            let (view, cx) = cx.add_window_view(|window, cx| {
                let mut workspace = Workspace::new(Some(root.into()), window, cx);
                workspace
                    ._subscriptions
                    .push(cx.observe(&cx.entity(), move |this, _, _| {
                        let mut times = recorded.borrow_mut();
                        if this.vault.is_some() {
                            times[0].get_or_insert_with(|| start.elapsed());
                        }
                        if !this.tabs.is_empty() {
                            times[1].get_or_insert_with(|| start.elapsed());
                        }
                        if this.vault.as_ref().is_some_and(|v| v.content_indexed()) {
                            times[2].get_or_insert_with(|| start.elapsed());
                        }
                    }));
                workspace
            });
            let count = view.read_with(cx, |this, _| this.vault.as_ref().unwrap().notes.len());
            println!(
                "simulated UI startup [vault available, first note, indexed]: {:?}; notes: {count}",
                milestones.borrow()
            );
            let start = std::time::Instant::now();
            for _ in 0..10 {
                view.update(cx, |_, cx| cx.notify());
                cx.run_until_parked();
            }
            println!("10 simulated UI redraws: {:?}", start.elapsed());
            if count > 100 {
                let start = std::time::Instant::now();
                view.update(cx, |this, cx| {
                    this.navigation.expanded = this
                        .vault
                        .as_ref()
                        .unwrap()
                        .folders
                        .iter()
                        .cloned()
                        .collect();
                    this.tree.flatten(&this.navigation.expanded);
                    let count = this.tree.visible.len();
                    this.notes_scroll
                        .scroll_to_item(count - 1, gpui::ScrollStrategy::Bottom);
                    cx.notify();
                });
                cx.run_until_parked();
                println!(
                    "expand every folder + scroll to last entry: {:?}",
                    start.elapsed()
                );
                view.read_with(cx, |this, _| {
                    let scroll = this.notes_scroll.0.borrow();
                    let size = scroll.last_item_size.unwrap();
                    let bottom_offset = size.contents.height - size.item.height;
                    assert!(bottom_offset > gpui::px(0.));
                    assert!((-scroll.base_handle.offset().y - bottom_offset).abs() < gpui::px(1.));
                });
            }
        }

        #[gpui::test]
        fn search_starts_after_first_paint_with_preloaded_vault(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("A.md"), "already loaded contents").unwrap();
            let vault = opalglass::vault::Vault::open(folder.path()).unwrap();
            assert!(vault.content_indexed());
            let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(None, w, cx));
            view.update_in(cx, |this, w, cx| {
                this.finish_open(Ok((vault, Default::default())), w, cx);
            });
            cx.run_until_parked();
            view.read_with(cx, |this, _| assert!(this.search_ready(), "{}", this.status));
        }

        #[gpui::test]
        fn indexed_search_ranks_typos_highlights_and_loads_only_visible_excerpts(
            cx: &mut TestAppContext,
        ) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(
                folder.path().join("Glossary.md"),
                "Glossary definitions and quartz.",
            )
            .unwrap();
            std::fs::write(folder.path().join("Repeated.md"), "foo foo foo").unwrap();
            for i in 0..60 {
                std::fs::write(
                    folder.path().join(format!("Note {i:02}.md")),
                    format!("quartz sample number {i}."),
                )
                .unwrap();
            }
            let (view, cx) =
                cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
            cx.run_until_parked();
            cx.simulate_keystrokes("ctrl-p");
            cx.simulate_input("glossry");
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            cx.run_until_parked();
            view.read_with(cx, |this, _| {
                assert!(this.search_ready(), "{}", this.status);
                assert_eq!(this.search_results[0].title, "Glossary");
                let snippet = this.search_results[0].snippet.as_ref().unwrap();
                assert!(
                    snippet
                        .highlights
                        .iter()
                        .any(|r| &snippet.fragment[r.clone()] == "Glossary")
                );
            });
            cx.simulate_keystrokes("ctrl-a");
            cx.simulate_input("quartz");
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            cx.run_until_parked();
            view.read_with(cx, |this, _| {
                assert_eq!(this.search_results.len(), 61);
                let loaded = this
                    .search_results
                    .iter()
                    .filter(|r| r.snippet.is_some())
                    .count();
                assert!(loaded > 0 && loaded < 20, "{loaded} snippets for viewport");
            });
            view.update(cx, |this, cx| {
                this.results_scroll
                    .scroll_to_item(60, gpui::ScrollStrategy::Center);
                this.sidebar.update(cx, |_, cx| cx.notify());
            });
            cx.run_until_parked();
            view.read_with(cx, |this, _| {
                assert!(this.search_results[60].snippet.is_some())
            });
            cx.simulate_keystrokes("down enter");
            cx.run_until_parked();
            view.read_with(cx, |this, _| {
                assert_eq!(
                    this.tabs[this.active].path,
                    this.search_results[this.search_selected].path
                )
            });
            cx.simulate_keystrokes("ctrl-p ctrl-a");
            cx.simulate_input("\"foo foo\"");
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            cx.run_until_parked();
            view.read_with(cx, |this, _| {
                assert_eq!(
                    this.search_results[0].snippet.as_ref().unwrap().highlights,
                    vec![0..11]
                )
            });
        }

        #[gpui::test]
        fn indexed_search_tracks_saved_external_renamed_and_removed_notes(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("A.md"), "alpha").unwrap();
            let (view, cx) =
                cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
            cx.run_until_parked();
            view.update(cx, |this, cx| {
                this.tabs[0]
                    .editor
                    .update(cx, |e, cx| e.set_text("nebula", cx));
                assert!(this.save_tab(0, cx));
            });
            cx.run_until_parked();
            cx.simulate_keystrokes("ctrl-p");
            cx.simulate_input("nebula");
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            cx.run_until_parked();
            view.read_with(cx, |this, _| {
                assert_eq!(this.search_results[0].path, PathBuf::from("A.md"))
            });
            std::fs::rename(folder.path().join("A.md"), folder.path().join("Moved.md")).unwrap();
            std::fs::write(folder.path().join("Moved.md"), "supernova").unwrap();
            cx.executor()
                .advance_clock(std::time::Duration::from_secs(3));
            cx.run_until_parked();
            cx.simulate_keystrokes("ctrl-p ctrl-a");
            cx.simulate_input("supernova");
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            cx.run_until_parked();
            view.read_with(cx, |this, _| {
                assert!(
                    !this.search_results.is_empty(),
                    "status={} ready={} rev={} index={} searching={} query={} paths={:?}",
                    this.status,
                    this.search_ready(),
                    this.revision,
                    this.index_revision,
                    this.searching,
                    this.search_query,
                    this.vault
                        .as_ref()
                        .unwrap()
                        .notes
                        .iter()
                        .map(|n| &n.path)
                        .collect::<Vec<_>>()
                );
                assert_eq!(this.search_results[0].path, PathBuf::from("Moved.md"));
            });
            std::fs::remove_file(folder.path().join("Moved.md")).unwrap();
            cx.executor()
                .advance_clock(std::time::Duration::from_secs(3));
            cx.run_until_parked();
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            cx.run_until_parked();
            view.read_with(cx, |this, _| assert!(this.search_results.is_empty()));
        }

        #[gpui::test]
        fn background_search_discards_old_queries_and_enter_waits_for_results(
            cx: &mut TestAppContext,
        ) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("A.md"), "alpha").unwrap();
            std::fs::write(folder.path().join("B.md"), "beta").unwrap();
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            cx.simulate_keystrokes("ctrl-p");
            cx.simulate_input("alpha");
            let cancelled = view.read_with(cx, |this, _| this.search_cancelled.clone());
            cx.simulate_keystrokes("ctrl-a");
            cx.simulate_input("beta");
            cx.simulate_keystrokes("enter");
            view.read_with(cx, |this, _| {
                assert!(this.searching);
                assert!(this.open_search_result);
                assert!(this.search_results.is_empty());
            });
            assert!(cancelled.load(std::sync::atomic::Ordering::Relaxed));
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(150));
            cx.run_until_parked();
            view.read_with(cx, |this, _| {
                assert!(!this.searching);
                assert_eq!(this.search_results.len(), 1);
                assert_eq!(this.tabs[this.active].path, PathBuf::from("B.md"));
            });
        }

        #[gpui::test]
        fn loading_vault_keeps_ui_available_and_preserves_intervening_edits(
            cx: &mut TestAppContext,
        ) {
            let old = tempfile::tempdir().unwrap();
            let next = tempfile::tempdir().unwrap();
            std::fs::write(old.path().join("Old.md"), "old").unwrap();
            std::fs::write(next.path().join("New.md"), "new").unwrap();
            let (view, cx) = cx
                .add_window_view(|window, cx| Workspace::new(Some(old.path().into()), window, cx));
            view.update_in(cx, |this, window, cx| {
                this.open_vault(next.path().into(), window, cx);
                // Loading hasn't run yet: opening returned without scanning.
                assert!(this.loading);
                assert_eq!(this.tabs[0].path, PathBuf::from("Old.md"));
                this.tabs[0]
                    .editor
                    .update(cx, |editor, cx| editor.set_text("intervening edit", cx));
            });
            cx.run_until_parked();
            assert_eq!(
                std::fs::read_to_string(old.path().join("Old.md")).unwrap(),
                "intervening edit"
            );
            view.read_with(cx, |this, _| {
                assert!(!this.loading);
                assert_eq!(this.tabs[0].path, PathBuf::from("New.md"));
            });
        }

        #[gpui::test]
        fn latest_vault_request_wins_and_load_conflicts_preserve_the_old_tab(
            cx: &mut TestAppContext,
        ) {
            let old = tempfile::tempdir().unwrap();
            let next = tempfile::tempdir().unwrap();
            std::fs::write(old.path().join("Old.md"), "old").unwrap();
            std::fs::write(next.path().join("New.md"), "new").unwrap();
            let (view, cx) = cx
                .add_window_view(|window, cx| Workspace::new(Some(old.path().into()), window, cx));
            view.update_in(cx, |this, window, cx| {
                this.open_vault(next.path().into(), window, cx);
                this.open_vault(old.path().into(), window, cx);
            });
            cx.run_until_parked();
            view.read_with(cx, |this, _| {
                assert_eq!(this.tabs[0].path, PathBuf::from("Old.md"))
            });
            view.update_in(cx, |this, window, cx| {
                this.open_vault(next.path().into(), window, cx);
                this.tabs[0]
                    .editor
                    .update(cx, |editor, cx| editor.set_text("local", cx));
                std::fs::write(old.path().join("Old.md"), "external").unwrap();
            });
            cx.run_until_parked();
            view.read_with(cx, |this, cx| {
                assert!(this.error);
                assert_eq!(this.tabs[0].path, PathBuf::from("Old.md"));
                assert_eq!(this.tabs[0].editor.read(cx).buffer.text(), "local");
            });
            assert_eq!(
                std::fs::read_to_string(old.path().join("Old.md")).unwrap(),
                "external"
            );
        }

        #[gpui::test]
        fn edit_save_search_link_and_conflict(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("Start.md"), "# Start\n\n[[Target]]").unwrap();
            std::fs::write(folder.path().join("Target.md"), "# Target").unwrap();
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            cx.simulate_keystrokes("ctrl-end");
            cx.simulate_input("\nA new thought 🦀");
            cx.simulate_keystrokes("ctrl-s");
            assert!(
                std::fs::read_to_string(folder.path().join("Start.md"))
                    .unwrap()
                    .ends_with("A new thought 🦀")
            );
            cx.simulate_keystrokes("ctrl-z ctrl-s");
            assert_eq!(
                std::fs::read_to_string(folder.path().join("Start.md")).unwrap(),
                "# Start\n\n[[Target]]\nA new thought "
            );
            view.update_in(cx, |this, window, cx| {
                this.follow("Target".into(), window, cx)
            });
            cx.run_until_parked();
            view.read_with(cx, |this, cx| {
                assert_eq!(this.tabs[this.active].path, PathBuf::from("Target.md"));
                assert_eq!(
                    this.vault
                        .as_ref()
                        .unwrap()
                        .backlinks(&this.tabs[this.active].path)
                        .len(),
                    1
                );
                assert!(!this.tabs[this.active].dirty(cx));
            });
            cx.simulate_keystrokes("ctrl-end");
            cx.simulate_input(" unsaved");
            std::fs::write(folder.path().join("Target.md"), "external edit").unwrap();
            cx.simulate_keystrokes("ctrl-s");
            view.read_with(cx, |this, cx| {
                assert!(this.error);
                assert!(this.tabs[this.active].dirty(cx));
                assert!(
                    this.tabs[this.active]
                        .editor
                        .read(cx)
                        .buffer
                        .text()
                        .ends_with("unsaved")
                );
            });
            assert_eq!(
                std::fs::read_to_string(folder.path().join("Target.md")).unwrap(),
                "external edit"
            );
            view.update(cx, |this, cx| assert!(!this.save_all(cx)));
            cx.simulate_keystrokes("ctrl-p");
            cx.simulate_input("Start");
            cx.simulate_keystrokes("enter");
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(150));
            cx.run_until_parked();
            view.read_with(cx, |this, _| {
                assert_eq!(this.tabs[this.active].path, PathBuf::from("Start.md"))
            });
        }

        #[gpui::test]
        fn create_unicode_note_and_clipboard_undo(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            cx.simulate_keystrokes("ctrl-n");
            cx.simulate_input("Ideas/İstanbul");
            cx.simulate_keystrokes("enter");
            cx.simulate_input("Hello 👩‍💻");
            cx.simulate_keystrokes("ctrl-a ctrl-c end enter ctrl-v ctrl-s");
            assert_eq!(
                std::fs::read_to_string(folder.path().join("Ideas/İstanbul.md")).unwrap(),
                "Hello 👩‍💻\nHello 👩‍💻"
            );
            cx.simulate_keystrokes("ctrl-z ctrl-s");
            assert_eq!(
                std::fs::read_to_string(folder.path().join("Ideas/İstanbul.md")).unwrap(),
                "Hello 👩‍💻\n"
            );
            cx.simulate_keystrokes("ctrl-shift-s");
            cx.simulate_input("Preserved");
            cx.simulate_keystrokes("enter");
            assert!(folder.path().join("Preserved.md").exists());
            view.read_with(cx, |this, _| assert_eq!(this.tabs.len(), 2));
        }

        #[gpui::test]
        fn refresh_does_not_revert_a_save_or_discard_dirty_edits(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            let path = folder.path().join("Note.md");
            std::fs::write(&path, "# Note\r\n").unwrap();
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            let old_scan = opalglass::vault::Vault::open(folder.path()).unwrap();
            let old_revision = view.read_with(cx, |this, _| this.revision);
            cx.simulate_keystrokes("ctrl-end");
            cx.simulate_input("Local");
            cx.simulate_keystrokes("enter ctrl-s");
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                "# Note\r\nLocal\r\n"
            );
            view.update(cx, |this, cx| this.apply_scan(old_scan, old_revision, cx));
            view.read_with(cx, |this, cx| {
                assert_eq!(
                    this.tabs[0].editor.read(cx).buffer.text(),
                    "# Note\nLocal\n"
                )
            });
            std::fs::write(&path, "External").unwrap();
            view.update(cx, |this, cx| {
                this.apply_scan(
                    opalglass::vault::Vault::open(folder.path()).unwrap(),
                    this.revision,
                    cx,
                )
            });
            cx.run_until_parked();
            view.read_with(cx, |this, cx| {
                assert_eq!(this.tabs[0].editor.read(cx).buffer.text(), "External")
            });
            cx.simulate_keystrokes("ctrl-end");
            cx.simulate_input(" dirty");
            std::fs::write(&path, "Another external edit").unwrap();
            view.update(cx, |this, cx| {
                this.apply_scan(
                    opalglass::vault::Vault::open(folder.path()).unwrap(),
                    this.revision,
                    cx,
                )
            });
            cx.run_until_parked();
            view.read_with(cx, |this, cx| {
                assert_eq!(this.tabs[0].editor.read(cx).buffer.text(), "External dirty");
                assert!(this.error);
            });
        }

        #[gpui::test]
        fn closing_last_tab_keeps_workspace_shortcuts_active(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("Note.md"), "# Note").unwrap();
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            cx.simulate_keystrokes("ctrl-w ctrl-n");
            cx.simulate_input("Second");
            cx.simulate_keystrokes("enter");
            cx.simulate_input("Still writing");
            cx.simulate_keystrokes("ctrl-s");
            assert_eq!(
                std::fs::read_to_string(folder.path().join("Second.md")).unwrap(),
                "Still writing"
            );
            view.read_with(cx, |this, _| assert_eq!(this.tabs.len(), 1));
        }

        #[gpui::test]
        fn copy_dialog_keeps_source_tab_open_and_preserves_line_endings(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("Note.md"), "# Note\r\nText\r\n").unwrap();
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            cx.simulate_keystrokes("ctrl-shift-s ctrl-w ctrl-p");
            view.read_with(cx, |this, _| assert_eq!(this.tabs.len(), 1));
            cx.simulate_input("Copy");
            cx.simulate_keystrokes("enter");
            assert_eq!(
                std::fs::read_to_string(folder.path().join("Copy.md")).unwrap(),
                "# Note\r\nText\r\n"
            );
            view.read_with(cx, |this, _| assert_eq!(this.tabs.len(), 2));
        }

        #[gpui::test]
        fn blocked_quit_focuses_the_conflicting_note(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("A.md"), "A").unwrap();
            std::fs::write(folder.path().join("B.md"), "B").unwrap();
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            cx.simulate_keystrokes("ctrl-end");
            cx.simulate_input(" dirty");
            view.update_in(cx, |this, window, cx| this.follow("B".into(), window, cx));
            cx.run_until_parked();
            std::fs::write(folder.path().join("A.md"), "external").unwrap();
            cx.simulate_keystrokes("ctrl-q");
            cx.simulate_input(" more");
            view.read_with(cx, |this, cx| {
                assert_eq!(this.tabs[this.active].path, PathBuf::from("A.md"));
                assert_eq!(
                    this.tabs[this.active].editor.read(cx).buffer.text(),
                    "A dirty more"
                );
                assert_eq!(this.tabs[1].editor.read(cx).buffer.text(), "B");
            });
        }

        #[gpui::test]
        fn reading_mode_keeps_workspace_shortcuts_available(cx: &mut TestAppContext) {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("Note.md"), "# Note").unwrap();
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            cx.simulate_keystrokes("ctrl-e ctrl-n");
            view.read_with(cx, |this, _| {
                assert!(this.mode == super::ViewMode::Preview);
                assert!(this.modal == super::Modal::New);
            });
            cx.simulate_keystrokes("escape ctrl-e");
            view.read_with(cx, |this, _| assert!(this.mode == super::ViewMode::Source));
        }
    }
}
