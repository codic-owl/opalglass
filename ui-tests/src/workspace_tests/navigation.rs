use super::*;

#[gpui::test]
fn history_restores_positions_reopens_closed_notes_and_branches(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    for name in ["A.md", "B.md", "C.md"] {
        std::fs::write(folder.path().join(name), "paragraph\n\n".repeat(400)).unwrap();
    }
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    view.update(cx, |this, cx| {
        this.tabs[0].editor.update(cx, |e, cx| {
            e.restore_position(
                33,
                36,
                opalglass::navigation::ScrollPosition {
                    row: 200,
                    offset: 2.,
                },
                cx,
            )
        })
    });
    cx.run_until_parked();
    view.update_in(cx, |this, w, cx| this.open_note("B.md".into(), w, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("alt-left");
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        assert_eq!(tab.path, PathBuf::from("A.md"));
        assert_eq!(
            (
                tab.editor.read(cx).buffer.anchor(),
                tab.editor.read(cx).buffer.cursor()
            ),
            (33, 36)
        );
        assert_eq!(tab.editor.read(cx).saved_scroll().row, 200);
    });
    cx.simulate_keystrokes("alt-right");
    view.update_in(cx, |this, w, cx| {
        let a = this
            .tabs
            .iter()
            .position(|t| t.path == PathBuf::from("A.md"))
            .unwrap();
        this.close_tab_at(a, w, cx);
    });
    cx.simulate_keystrokes("alt-left");
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[this.active].path, PathBuf::from("A.md"));
        assert_eq!(this.tabs[this.active].editor.read(cx).buffer.cursor(), 36);
    });
    let point = cx
        .debug_bounds("tabs")
        .map(|b| b.center())
        .unwrap_or(gpui::point(gpui::px(400.), gpui::px(15.)));
    cx.simulate_mouse_down(
        point,
        gpui::MouseButton::Navigate(gpui::NavigationDirection::Forward),
        gpui::Modifiers::default(),
    );
    view.read_with(cx, |this, _| {
        assert_eq!(this.tabs[this.active].path, PathBuf::from("B.md"))
    });
    cx.simulate_keystrokes("alt-left");
    view.update_in(cx, |this, w, cx| this.open_note("C.md".into(), w, cx));
    cx.run_until_parked();
    view.read_with(cx, |this, _| assert!(this.history_forward.is_empty()));
}

#[gpui::test]
fn note_find_outline_and_preferences_work_through_controls(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let text = "# One\nAlpha éclair ALPHA\n\n## Two\nAlpha again\n";
    std::fs::write(folder.path().join("A.md"), text).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-f");
    cx.simulate_input("alpha");
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert_eq!(this.find.matches.len(), 3);
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.selected(),
            "Alpha"
        );
        assert!(this.search.read(cx).buffer.text().is_empty());
    });
    cx.simulate_keystrokes("enter enter enter");
    view.read_with(cx, |this, _| assert_eq!(this.find.selected, Some(0)));
    cx.simulate_keystrokes("shift-enter");
    view.read_with(cx, |this, _| assert_eq!(this.find.selected, Some(2)));
    cx.simulate_keystrokes("escape ctrl-shift-o");
    cx.run_until_parked();
    click(cx, "outline-heading-1");
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.cursor(),
            text.find("## Two").unwrap()
        )
    });
    view.update(cx, |this, _| {
        this.state_directory = Some(state.path().into())
    });
    cx.simulate_keystrokes("ctrl-,");
    click(cx, "font-larger");
    click(cx, "setting-line-numbers");
    click(cx, "setting-default-editor");
    cx.simulate_keystrokes("escape");
    view.read_with(cx, |this, cx| {
        assert!(!this.settings_open);
        assert_eq!(this.preferences.font_size, 15.);
        assert!(!this.tabs[this.active].editor.read(cx).line_numbers);
        assert_eq!(
            opalglass::state::Preferences::load(state.path()),
            this.preferences
        );
    });
}

#[gpui::test]
fn link_completion_inserts_resolvable_target_and_undo(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("Nested")).unwrap();
    for (path, text) in [
        ("A.md", ""),
        ("Target.md", "root"),
        ("Nested/Target.md", "nested"),
    ] {
        std::fs::write(folder.path().join(path), text).unwrap();
    }
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    cx.simulate_input("[[Tar");
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert_eq!(this.completion.candidates.len(), 2)
    });
    // Choose the nested duplicate by mouse: it must not resolve to the root note.
    let i = view.read_with(cx, |this, _| {
        this.completion
            .candidates
            .iter()
            .position(|(p, _)| p == &PathBuf::from("Nested/Target.md"))
            .unwrap()
    });
    click(
        cx,
        if i == 0 {
            "link-option-0"
        } else {
            "link-option-1"
        },
    );
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.text(),
            "[[/Nested/Target]]"
        );
    });
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.text(),
            "[[Tar"
        )
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("down enter");
    view.read_with(cx, |this, cx| {
        assert!(
            this.tabs[this.active]
                .editor
                .read(cx)
                .buffer
                .text()
                .ends_with("]]")
        )
    });
}

#[gpui::test]
fn session_restores_order_active_selection_and_scroll_skipping_missing(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    for name in ["A.md", "B.md", "C.md"] {
        std::fs::write(
            folder.path().join(name),
            "# Heading\n\nParagraph with words\n\n".repeat(500),
        )
        .unwrap();
    }
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    for name in ["B.md", "C.md"] {
        view.update_in(cx, |this, w, cx| this.open_note(name.into(), w, cx));
        cx.run_until_parked();
    }
    view.update_in(cx, |this, w, cx| {
        this.activate_tab(1, w, cx);
        this.state_directory = Some(state.path().into());
        this.tabs[1].editor.update(cx, |e, cx| {
            e.restore_position(
                14,
                20,
                opalglass::navigation::ScrollPosition {
                    row: 420,
                    offset: 3.,
                },
                cx,
            )
        });
        this.tabs[1].preview_scroll.scroll_to(gpui::ListOffset {
            item_ix: 250,
            offset_in_item: gpui::px(2.),
        });
    });
    cx.run_until_parked();
    let saved = view.update(cx, |this, cx| {
        this.persist_navigation(cx);
        this.navigation.clone()
    });
    assert_eq!(saved.tabs[1].source.row, 420);
    std::fs::remove_file(folder.path().join("C.md")).unwrap();
    view.update_in(cx, |this, w, cx| {
        let vault = opalglass::vault::Vault::open(folder.path()).unwrap();
        let saved = opalglass::navigation::State::load(state.path(), &vault.root);
        this.finish_open(Ok((vault, saved)), w, cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs.iter().map(|t| t.path.clone()).collect::<Vec<_>>(),
            vec![PathBuf::from("A.md"), PathBuf::from("B.md")]
        );
        assert_eq!(this.active, 1);
        let editor = this.tabs[1].editor.read(cx);
        assert_eq!((editor.buffer.anchor(), editor.buffer.cursor()), (14, 20));
        assert_eq!(editor.saved_scroll(), saved.tabs[1].source);
        assert_eq!(
            this.tabs[1].preview_scroll.logical_scroll_top().item_ix,
            saved.tabs[1].preview.row
        );
    });
}

#[gpui::test]
fn missing_note_links_update_after_vault_changes_and_edits(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(folder.path().join("Folder")).unwrap();
    let source = "# Note\n[[Known]] [[Later#Heading|未作成]] [[#Note]] `[[Code]]`\n\n```\n[[Fenced]]\n```\n[Website](https://example.com)";
    std::fs::write(folder.path().join("Folder/Note.md"), source).unwrap();
    std::fs::write(folder.path().join("Folder/Known.md"), "known").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
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

#[gpui::test]
fn tab_press_activates_and_repeated_open_reuses_views(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(
        folder.path().join("A.md"),
        (0..150).map(|i| format!("#tag{i} ")).collect::<String>(),
    )
    .unwrap();
    std::fs::write(folder.path().join("B.md"), "[[A]]").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
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
fn tab_click_reveals_note_and_preserves_document_focus(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(folder.path().join("Nested/Deep")).unwrap();
    std::fs::write(folder.path().join("Welcome.md"), "welcome").unwrap();
    std::fs::write(folder.path().join("Nested/Deep/Note.md"), "body").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    assert!(cx.debug_bounds("vault-location").is_none());
    assert!(cx.debug_bounds("reveal-active").is_none());
    view.update_in(cx, |this, w, cx| {
        this.open_note("Nested/Deep/Note.md".into(), w, cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-p");
    cx.simulate_input("Welcome");
    cx.simulate_keystrokes("ctrl-b");
    click(cx, "tab-1"); // Re-click the active tab with search and the sidebar hidden.
    view.read_with(cx, |this, cx| {
        assert!(!this.navigation.hidden);
        assert!(this.search.read(cx).buffer.text().is_empty());
        assert_eq!(this.navigation.selected, Some("Nested/Deep/Note.md".into()));
        assert!(this.navigation.expanded.contains(&PathBuf::from("Nested")));
        assert!(
            this.navigation
                .expanded
                .contains(&PathBuf::from("Nested/Deep"))
        );
    });
    assert!(cx.debug_bounds("entry:Nested/Deep/Note.md").is_some());
    cx.simulate_input("typed ");
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[1].editor.read(cx).buffer.text(), "typed body")
    });
    click(cx, "tab-0");
    view.read_with(cx, |this, _| {
        assert_eq!(this.active, 0);
        assert_eq!(this.navigation.selected, Some("Welcome.md".into()));
    });
}

#[gpui::test]
fn tab_drag_close_and_keyboard_preserve_active_document(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    for name in ["A", "B", "C"] {
        std::fs::write(folder.path().join(format!("{name}.md")), name).unwrap();
    }
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
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
fn mouse_moves_and_background_tab_close_preserve_conflicting_edits(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("Archive")).unwrap();
    std::fs::write(folder.path().join("A.md"), "original").unwrap();
    std::fs::write(folder.path().join("B.md"), "other").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
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
fn keyboard_tab_switching_reveals_tabs_in_narrow_window(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    for i in 0..12 {
        std::fs::write(folder.path().join(format!("Note{i:02}.md")), "text").unwrap();
    }
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
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
fn pending_note_loads_do_not_override_newer_navigation(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    for name in ["A", "B", "C"] {
        std::fs::write(folder.path().join(format!("{name}.md")), name).unwrap();
    }
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
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
fn loading_vault_keeps_ui_available_and_preserves_intervening_edits(cx: &mut TestAppContext) {
    let old = tempfile::tempdir().unwrap();
    let next = tempfile::tempdir().unwrap();
    std::fs::write(old.path().join("Old.md"), "old").unwrap();
    std::fs::write(next.path().join("New.md"), "new").unwrap();
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(old.path().into()), window, cx));
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
fn latest_vault_request_wins_and_load_conflicts_preserve_the_old_tab(cx: &mut TestAppContext) {
    let old = tempfile::tempdir().unwrap();
    let next = tempfile::tempdir().unwrap();
    std::fs::write(old.path().join("Old.md"), "old").unwrap();
    std::fs::write(next.path().join("New.md"), "new").unwrap();
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(old.path().into()), window, cx));
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
fn closing_last_tab_keeps_workspace_shortcuts_active(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("Note.md"), "# Note").unwrap();
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
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
