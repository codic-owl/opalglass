use super::*;

#[gpui::test]
fn saved_versions_follow_ui_moves_and_restore_into_the_renamed_tab(cx: &mut TestAppContext) {
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
    cx.run_until_parked();
    view.update_in(cx, |this, w, cx| {
        this.move_entry("A.md".into(), "Renamed.md".into(), w, cx)
    });
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert_eq!(this.tabs[this.active].path, PathBuf::from("Renamed.md"))
    });
    view.update_in(cx, |this, w, cx| this.open_history(w, cx));
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert_eq!(this.history_panel.as_ref().unwrap().versions.len(), 1)
    });
    cx.simulate_keystrokes("enter");
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.text(),
            "Original 👩‍💻\n"
        )
    });
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.text(),
            "Latest\n"
        )
    });
    assert!(!folder.path().join("A.md").exists());
    assert_eq!(
        std::fs::read_to_string(folder.path().join("Renamed.md")).unwrap(),
        "Latest\r\n"
    );
}
