use super::*;

#[gpui::test]
fn embedded_content_refreshes_from_saved_files_and_unsaved_inactive_tabs(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let host = "# Host\n\n![[Source#Part]]\n";
    let source = "# Part\n\n```txt\nOLD\n```\n";
    std::fs::write(folder.path().join("Host.md"), host).unwrap();
    std::fs::write(folder.path().join("Source.md"), source).unwrap();
    let (view, cx) = reading(cx, folder.path());
    let id = instances(&view, cx, "Source#Part")[0].id;
    show_instance(&view, cx, id);
    copy_code(&view, cx, id, "OLD\n");
    let saved = source.replace("OLD", "NEW");
    std::fs::write(folder.path().join("Source.md"), &saved).unwrap();
    view.update(cx, |this, cx| {
        this.apply_scan(
            opalglass::vault::Vault::open(folder.path()).unwrap(),
            this.revision,
            cx,
        );
    });
    settle(cx);
    copy_code(&view, cx, id, "NEW\n");
    view.update_in(cx, |this, w, cx| this.open_note("Source.md".into(), w, cx));
    settle(cx);
    // Leave before the editor's delayed analysis starts, reproducing a dirty
    // inactive source whose own Reading preparation still contains OLD/NEW.
    view.update_in(cx, |this, w, cx| {
        this.tabs[this.active].editor.update(cx, |editor, cx| {
            let at = editor.buffer.text().find("NEW").unwrap();
            editor.buffer.replace(at..at + 3, "UNSAVED");
            cx.notify();
        });
        this.open_note("Host.md".into(), w, cx);
    });
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(100));
    settle(cx);
    copy_code(&view, cx, id, "UNSAVED\n");
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        assert_eq!(tab.path, std::path::PathBuf::from("Host.md"));
        assert_eq!(tab.editor.read(cx).buffer.text(), host);
        assert!(!tab.dirty(cx));
        let source_tab = this
            .tabs
            .iter()
            .find(|tab| tab.path == std::path::PathBuf::from("Source.md"))
            .unwrap();
        assert!(source_tab.dirty(cx));
        assert!(source_tab.editor.read(cx).buffer.text().contains("UNSAVED"));
    });
    assert_eq!(
        std::fs::read_to_string(folder.path().join("Source.md")).unwrap(),
        saved
    );
}
