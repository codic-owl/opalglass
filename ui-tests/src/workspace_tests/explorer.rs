use super::*;

#[gpui::test]
fn explorer_trash_saves_dirty_note_restores_and_blocks_conflict(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "original").unwrap();
    std::fs::write(folder.path().join("B.md"), "[[A]]").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-end");
    cx.simulate_input(" saved on delete");
    right_click(cx, "entry:A.md");
    click(cx, "explorer-action-2");
    view.read_with(cx, |this, _| assert!(this.modal == super::Modal::Delete));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!folder.path().join("A.md").exists());
    view.read_with(cx, |this, _| assert!(this.tabs.is_empty()));
    right_click_explorer_space(cx);
    click(cx, "explorer-action-2"); // New note, New folder, Restore.
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
        "original saved on delete"
    );
    view.update_in(cx, |this, w, cx| this.open_note("A.md".into(), w, cx));
    cx.run_until_parked();
    cx.simulate_input("unsaved");
    std::fs::write(folder.path().join("A.md"), "external").unwrap();
    right_click(cx, "entry:A.md");
    click(cx, "explorer-action-2");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
        "external"
    );
    view.read_with(cx, |this, cx| {
        assert!(this.tabs[0].dirty(cx));
        assert!(this.error);
    });
}

#[gpui::test]
fn explorer_context_creation_inline_rename_and_root(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("Ideas")).unwrap();
    std::fs::write(folder.path().join("Welcome.md"), "[[Ideas/Draft]]").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
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
    right_click_explorer_space(cx);
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
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
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
    right_click(cx, "entry:Archive/Note.md");
    click(cx, "explorer-action-1"); // Move to the root through the chooser.
    cx.simulate_input("/");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
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
fn explorer_menu_keyboard_dismissal_and_name_collision(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "a").unwrap();
    std::fs::write(folder.path().join("B.md"), "b").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
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
    right_click_explorer_space(cx);
    cx.simulate_keystrokes("ctrl-n");
    cx.simulate_input("From shortcut");
    cx.simulate_keystrokes("enter");
    assert!(folder.path().join("From shortcut.md").exists());
}

#[gpui::test]
fn tree_keyboard_reveal_and_search_restore_exact_scroll(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(folder.path().join("Zebra/Empty")).unwrap();
    for i in 0..400 {
        std::fs::write(folder.path().join(format!("Zebra/Note{i:04}.md")), "body").unwrap();
    }
    std::fs::write(folder.path().join("Welcome.md"), "welcome").unwrap();
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
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
    cx.simulate_keystrokes("ctrl-p");
    cx.simulate_input("Note0001");
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(150));
    cx.run_until_parked();
    view.read_with(cx, |this, _| assert_eq!(this.search_results.len(), 1));
    cx.simulate_keystrokes("ctrl-a backspace");
    view.read_with(cx, |this, _| {
        assert!(
            (this.notes_scroll.0.borrow().base_handle.offset().y - offset.y).abs() < gpui::px(0.01)
        );
        assert!(this.navigation.expanded.contains(&PathBuf::from("Zebra")));
    });
}

#[gpui::test]
fn explorer_toolbar_creates_sorts_and_follows_navigation(cx: &mut TestAppContext) {
    use opalglass::navigation::SortOrder;
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(folder.path().join("Zed/Deep")).unwrap();
    std::fs::write(folder.path().join("Zed/Deep/Note.md"), "nested").unwrap();
    std::fs::write(folder.path().join("Welcome.md"), "welcome").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    let toolbar = cx.debug_bounds("explorer-toolbar").unwrap();
    assert!(toolbar.top() < gpui::px(1.));
    assert_eq!(
        cx.debug_bounds("sidebar-rail").unwrap().size.width,
        gpui::px(super::SIDEBAR_RAIL_WIDTH)
    );
    view.update(cx, |this, _| this.navigation.selected = Some("Zed".into()));
    click(cx, "explorer-new-folder");
    cx.simulate_input("Created");
    cx.simulate_keystrokes("enter");
    assert!(folder.path().join("Created").is_dir());
    click(cx, "explorer-new-note");
    cx.simulate_input("Toolbar");
    cx.simulate_keystrokes("enter");
    assert!(folder.path().join("Toolbar.md").is_file());
    click(cx, "explorer-sort");
    assert!(cx.debug_bounds("sort-menu").is_some());
    click(cx, "sort-option-1");
    view.read_with(cx, |this, _| {
        assert_eq!(this.navigation.sort, SortOrder::NameDescending);
        assert_eq!(this.tree.visible[0].name(), "Zed");
    });
    click(cx, "explorer-sort");
    cx.simulate_keystrokes("up enter");
    view.read_with(cx, |this, _| {
        assert_eq!(this.navigation.sort, SortOrder::NameAscending)
    });
    click(cx, "explorer-auto-reveal");
    view.update_in(cx, |this, w, cx| {
        this.open_note("Zed/Deep/Note.md".into(), w, cx)
    });
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert!(this.navigation.auto_reveal);
        assert_eq!(this.navigation.selected, Some("Zed/Deep/Note.md".into()));
        assert!(
            this.navigation
                .expanded
                .contains(&PathBuf::from("Zed/Deep"))
        );
    });
    click(cx, "explorer-collapse-all");
    view.update(cx, |_, cx| cx.notify()); // No immediate re-expansion of the same active note.
    cx.run_until_parked();
    view.read_with(cx, |this, _| assert!(this.navigation.expanded.is_empty()));
    click(cx, "explorer-collapse-all");
    view.read_with(cx, |this, _| assert_eq!(this.navigation.expanded.len(), 3));
    click(cx, "explorer-collapse-all");
    click(cx, "explorer-auto-reveal");
    cx.simulate_keystrokes("ctrl-1 ctrl-9");
    view.read_with(cx, |this, _| assert!(this.navigation.expanded.is_empty()));
    click(cx, "toggle-sidebar");
    view.read_with(cx, |this, _| assert!(this.navigation.hidden));
    assert_eq!(
        cx.debug_bounds("tab-0").unwrap().left(),
        gpui::px(super::SIDEBAR_RAIL_WIDTH)
    );
    assert!(cx.debug_bounds("sidebar-rail").is_some());
    click(cx, "toggle-sidebar");
    view.read_with(cx, |this, _| assert!(!this.navigation.hidden));
    assert!(cx.debug_bounds("tab-0").unwrap().left() > gpui::px(270.));
}

#[gpui::test]
fn explorer_toolbar_requires_a_vault(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(None, w, cx));
    for id in [
        "explorer-new-note",
        "explorer-new-folder",
        "explorer-sort",
        "explorer-auto-reveal",
        "explorer-collapse-all",
    ] {
        click(cx, id);
    }
    view.read_with(cx, |this, _| {
        assert!(this.modal == super::Modal::None);
        assert!(this.sort_menu.is_none());
        assert!(!this.navigation.auto_reveal);
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
    let resize_position = cx.debug_bounds("sidebar-resize").unwrap().center();
    cx.simulate_mouse_down(
        resize_position,
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.simulate_mouse_move(
        gpui::point(gpui::px(355. + super::SIDEBAR_RAIL_WIDTH), gpui::px(300.)),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.simulate_mouse_up(
        gpui::point(gpui::px(355. + super::SIDEBAR_RAIL_WIDTH), gpui::px(300.)),
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
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
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
fn move_keeps_window_open_and_blocks_document_edits_until_complete(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "original").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
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
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
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
fn virtual_note_list_opens_the_last_row_and_keeps_scroll_after_saving(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    for index in 0..400 {
        std::fs::write(folder.path().join(format!("Note{index:04}.md")), "note").unwrap();
    }
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
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
