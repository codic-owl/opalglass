use super::{TestAppContext, ViewMode, Workspace, click};

#[gpui::test]
fn reading_tasks_toggle_without_changing_bare_brackets(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let source = "- [ ] Todo\n- [x] Done\n\n[ ] Bare brackets\n";
    std::fs::write(folder.path().join("A.md"), source).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    let checked = source.replacen("[ ]", "[x]", 1);
    view.update_in(cx, |this, w, cx| this.set_mode(ViewMode::Preview, w, cx));
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        assert_eq!(
            tab.prepared
                .rows
                .iter()
                .filter(|row| row.task.is_some())
                .count(),
            2
        );
        assert_eq!(tab.editor.read(cx).buffer.text(), source);
    });
    click(cx, "reading-task-0");
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.text(),
            checked
        )
    });
    click(cx, "reading-task-0");
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[this.active].editor.read(cx).buffer.text(), source)
    });
    assert_eq!(
        std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
        source
    );
}
