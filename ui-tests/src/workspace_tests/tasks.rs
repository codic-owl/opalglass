use super::{TestAppContext, ViewMode, Workspace, click};

#[gpui::test]
fn reading_tasks_toggle_without_changing_bare_brackets(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let source = "- [ ] Todo\n- [x] Done\n\n[ ] Bare brackets\n";
    std::fs::write(folder.path().join("A.md"), source).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    let checked = source.replacen("[ ]", "[x]", 1);
    view.update_in(cx, |this, w, cx| this.set_mode(ViewMode::Preview, w, cx));
    // Match the established Reading click fixture and refresh cached selectors.
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(800.)));
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
    let task_bounds = cx.debug_bounds("reading-task-0").unwrap();
    let pane_bounds = cx.debug_bounds("preview-pane").unwrap();
    let column_bounds = cx.debug_bounds("preview-column-0").unwrap();
    click(cx, "reading-task-0");
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        assert_eq!(
            tab.editor.read(cx).buffer.text(),
            checked,
            "task={task_bounds:?} pane={pane_bounds:?} column={column_bounds:?} status={} source_revision={} prepared_revision={} task_range={:?}",
            this.status,
            tab.editor.read(cx).buffer.revision(),
            tab.prepared_revision,
            tab.prepared.rows[0].task,
        )
    });
    // Source-token guards reject clicks from the old preview until its existing
    // 75 ms preparation debounce completes; advance the simulated clock only.
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(100));
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        assert_eq!(tab.prepared_revision, tab.editor.read(cx).buffer.revision());
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
