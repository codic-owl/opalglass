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
            let start = cx.debug_bounds("empty-source").unwrap();
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
                assert_eq!(this.notes_scroll.0.borrow().base_handle.offset(), offset)
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
            cx.simulate_input("Archive/Renamed");
            cx.simulate_keystrokes("enter");
            view.read_with(cx, |this, cx| {
                assert!(!this.error, "{}", this.status);
                assert_eq!(
                    this.tabs[this.active].path,
                    PathBuf::from("Archive/Renamed.md")
                );
                assert!(!this.tabs[this.active].dirty(cx));
                assert_eq!(
                    this.vault
                        .as_ref()
                        .unwrap()
                        .backlinks(std::path::Path::new("Archive/Renamed.md"))
                        .len(),
                    1
                );
            });
            assert!(folder.path().join("Thoughts/Empty/Fresh.md").is_file());
            assert_eq!(
                std::fs::read_to_string(folder.path().join("Welcome.md")).unwrap(),
                "[[Archive/Renamed|Link]]"
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
                format!("{}\n# Destination\n", "paragraph\n\n".repeat(500)),
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
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("Start.md"), "start").unwrap();
            std::fs::write(folder.path().join("Target.md"), &text).unwrap();
            let (view, cx) = cx.add_window_view(|window, cx| {
                Workspace::new(Some(folder.path().into()), window, cx)
            });
            let start = std::time::Instant::now();
            view.update_in(cx, |this, window, cx| {
                this.open_note("Target.md".into(), window, cx)
            });
            cx.run_until_parked();
            println!("note bytes: {}; open: {:?}", text.len(), start.elapsed());
            let start = std::time::Instant::now();
            for _ in 0..3 {
                view.update(cx, |_, cx| cx.notify());
                cx.run_until_parked();
            }
            println!("3 note redraws: {:?}", start.elapsed());
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
            let (view, cx) =
                cx.add_window_view(|window, cx| Workspace::new(Some(root.into()), window, cx));
            let count = view.read_with(cx, |this, _| this.vault.as_ref().unwrap().notes.len());
            println!("simulated UI open: {:?}; notes: {count}", start.elapsed());
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
