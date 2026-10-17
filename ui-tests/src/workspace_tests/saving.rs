use super::*;

#[gpui::test]
fn external_comparison_rechecks_disk_and_preserves_both_versions_until_resolved(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("A.md");
    std::fs::write(&path, "# Title\nBase text\n").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    view.update(cx, |this, cx| {
        this.tabs[this.active]
            .editor
            .update(cx, |e, cx| e.set_text("# Title\nLocal text\n", cx))
    });
    std::fs::write(&path, "# Title\nRemote text\n").unwrap();
    view.update(cx, |this, cx| {
        let mut vault = this.vault.clone().unwrap();
        vault.refresh().unwrap();
        this.apply_scan(vault, this.revision, cx);
    });
    click(cx, "compare-external");
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        use opalglass::comparison::Kind;
        let panel = this.comparison.as_ref().unwrap();
        assert!(
            panel
                .rows
                .iter()
                .any(|r| r.kind == Kind::Removed && r.text == "Local text")
        );
        assert!(
            panel
                .rows
                .iter()
                .any(|r| r.kind == Kind::Added && r.text == "Remote text")
        );
        assert!(
            panel
                .disk_changes
                .iter()
                .any(|r| r.kind == Kind::Removed && r.text == "Base text")
        );
    });
    std::fs::write(&path, "Changed again\n").unwrap();
    click(cx, "comparison-save-ours");
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "Changed again\n");
    view.read_with(cx, |this, cx| {
        assert!(
            this.comparison
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .contains("changed again")
        );
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.text(),
            "# Title\nLocal text\n"
        );
    });
    click(cx, "comparison-refresh");
    cx.run_until_parked();
    click(cx, "comparison-save-ours");
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# Title\nLocal text\n"
    );
    view.read_with(cx, |this, cx| {
        assert!(this.comparison.is_none());
        assert!(!this.tabs[this.active].dirty(cx));
    });
    view.update(cx, |this, cx| {
        this.tabs[this.active]
            .editor
            .update(cx, |e, cx| e.set_text("Unsaved again\n", cx))
    });
    std::fs::write(&path, "Remote again\n").unwrap();
    view.update_in(cx, |this, window, cx| this.open_comparison(window, cx));
    cx.run_until_parked();
    click(cx, "comparison-use-disk");
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.text(),
            "Remote again\n"
        )
    });
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.text(),
            "Unsaved again\n"
        );
        assert!(this.tabs[this.active].dirty(cx));
    });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "Remote again\n");
}

#[gpui::test]
fn periodic_autosave_preserves_edits_during_write_and_conflicts(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "original").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    view.update(cx, |this, _| {
        this.state_directory = Some(state.path().into())
    });
    cx.simulate_keystrokes("ctrl-end");
    cx.simulate_input(" first");
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(3));
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
        "original first"
    );
    view.read_with(cx, |this, cx| assert!(!this.tabs[0].dirty(cx)));
    cx.simulate_input(" second");
    view.update(cx, |this, cx| {
        this.checkpoint(cx);
        this.tabs[0].editor.update(cx, |e, cx| {
            e.buffer.insert(" newer");
            cx.notify();
        });
    });
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
        "original first second"
    );
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[0].editor.read(cx).buffer.text(),
            "original first second newer"
        );
        assert!(this.tabs[0].dirty(cx));
    });
    std::fs::write(folder.path().join("A.md"), "external version").unwrap();
    view.update(cx, |this, cx| this.checkpoint(cx));
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
        "external version"
    );
    view.read_with(cx, |this, cx| {
        assert!(this.error);
        assert!(this.tabs[0].dirty(cx));
        let recovery = opalglass::state::Recovery::load(
            state.path(),
            &this.vault.as_ref().unwrap().root,
            std::path::Path::new("A.md"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(recovery.text, "original first second newer");
    });
}

#[gpui::test]
fn autosave_off_recovery_survives_restart_and_reload_discards_it(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "original").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    view.update(cx, |this, _| {
        this.state_directory = Some(state.path().into());
        this.preferences.autosave = false;
    });
    cx.simulate_keystrokes("ctrl-end");
    cx.simulate_input(" unsaved");
    view.update(cx, |this, cx| {
        this.persist_navigation(cx);
        this.checkpoint(cx);
    });
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
        "original"
    );
    view.update_in(cx, |this, w, cx| {
        let vault = opalglass::vault::Vault::open(folder.path()).unwrap();
        let saved = opalglass::navigation::State::load(state.path(), &vault.root);
        this.finish_open(Ok((vault, saved)), w, cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[0].editor.read(cx).buffer.text(),
            "original unsaved"
        );
        assert!(this.tabs[0].dirty(cx));
    });
    view.update(cx, |this, cx| this.reload_tab(0, cx));
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[0].editor.read(cx).buffer.text(), "original");
        assert!(
            opalglass::state::Recovery::load(
                state.path(),
                &this.vault.as_ref().unwrap().root,
                std::path::Path::new("A.md")
            )
            .unwrap()
            .is_none()
        );
    });
}

#[gpui::test]
fn saved_versions_restore_into_editor_and_remain_undoable(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "Original 👩‍💻\r\n").unwrap();
    let mut vault = opalglass::vault::Vault::open(folder.path()).unwrap();
    vault
        .save(
            std::path::Path::new("A.md"),
            "Original 👩‍💻\r\n",
            "Latest\r\n",
        )
        .unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(800.)));
    cx.run_until_parked();
    view.update_in(cx, |this, w, cx| {
        this.execute_command(opalglass::commands::Command::History, w, cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    view.read_with(cx, |this, cx| {
        assert!(this.history_panel.is_none());
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.text(),
            "Original 👩‍💻\n"
        );
    });
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.text(),
            "Latest\n"
        )
    });
}

#[gpui::test]
fn disk_reload_preserves_edits_made_after_the_request(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "original").unwrap();
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
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
fn edit_save_search_link_and_conflict(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("Start.md"), "# Start\n\n[[Target]]").unwrap();
    std::fs::write(folder.path().join("Target.md"), "# Target").unwrap();
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
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
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
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
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
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
fn copy_dialog_keeps_source_tab_open_and_preserves_line_endings(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("Note.md"), "# Note\r\nText\r\n").unwrap();
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
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
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
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
