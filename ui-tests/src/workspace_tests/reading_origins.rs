//! Shared Reading widgets resolve actions against their own source document.
use super::super::reading_context::ReadingContext;
use super::*;

#[gpui::test]
fn reading_links_and_missing_styles_resolve_from_the_document_origin(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("Nested")).unwrap();
    std::fs::write(folder.path().join("Host.md"), "Host").unwrap();
    std::fs::write(folder.path().join("Target.md"), "# Root heading").unwrap();
    std::fs::write(
        folder.path().join("Nested/Source.md"),
        "[target](Target.md#Nested%20heading)",
    )
    .unwrap();
    std::fs::write(folder.path().join("Nested/Target.md"), "# Nested heading").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    view.update_in(cx, |this, w, cx| {
        this.open_note("Nested/Source.md".into(), w, cx)
    });
    cx.run_until_parked();
    let mut context = view.read_with(cx, |this, _| ReadingContext::host(&this.tabs[this.active]));
    context.instance = 7;
    view.update_in(cx, |this, w, cx| this.open_note("Host.md".into(), w, cx));
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert!(
            !this
                .link_highlight_from(&context.path, "Target.md#Nested%20heading")
                .underline
                .unwrap()
                .wavy
        );
        assert!(
            this.link_highlight("Target.md#Nested%20heading")
                .underline
                .unwrap()
                .wavy
        );
    });
    view.update_in(cx, |this, w, cx| {
        this.follow_reading(context, "Target.md#Nested%20heading".into(), w, cx)
    });
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert_eq!(
            this.tabs[this.active].path,
            PathBuf::from("Nested/Target.md")
        );
    });
}

#[gpui::test]
fn reading_tasks_edit_the_matching_source_tab_and_keep_its_undo_history(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let host = "- [ ] Host task\n";
    let source = "- [ ] Source task **é 👩‍💻**\n";
    std::fs::write(folder.path().join("Host.md"), host).unwrap();
    std::fs::write(folder.path().join("Source.md"), source).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    view.update_in(cx, |this, w, cx| this.open_note("Source.md".into(), w, cx));
    cx.run_until_parked();
    let (mut context, task) = view.read_with(cx, |this, _| {
        let context = ReadingContext::host(&this.tabs[this.active]);
        let task = context
            .prepared
            .rows
            .iter()
            .find_map(|row| row.task.clone())
            .unwrap();
        (context, task)
    });
    context.instance = 8;
    view.update_in(cx, |this, w, cx| this.open_note("Host.md".into(), w, cx));
    cx.run_until_parked();
    view.update_in(cx, |this, w, cx| {
        this.toggle_reading_task(context.clone(), task.clone(), false, w, cx)
    });
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[this.active].path, PathBuf::from("Host.md"));
        assert_eq!(this.tabs[this.active].editor.read(cx).buffer.text(), host);
        let source_tab = this
            .tabs
            .iter()
            .find(|tab| tab.path == PathBuf::from("Source.md"))
            .unwrap();
        assert_eq!(
            source_tab.editor.read(cx).buffer.text(),
            source.replacen("[ ]", "[x]", 1)
        );
        assert!(source_tab.dirty(cx));
        assert!(!this.tabs[this.active].dirty(cx));
    });
    assert_eq!(
        std::fs::read_to_string(folder.path().join("Source.md")).unwrap(),
        source
    );
    // The old widget's source token is now stale. It may open the source for
    // review, but must not apply another edit to either note.
    view.update_in(cx, |this, w, cx| {
        this.toggle_reading_task(context, task, false, w, cx);
        this.set_mode(ViewMode::Source, w, cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[this.active].path, PathBuf::from("Source.md"));
        assert_eq!(this.tabs[this.active].editor.read(cx).buffer.text(), source);
        let host_tab = this
            .tabs
            .iter()
            .find(|tab| tab.path == PathBuf::from("Host.md"))
            .unwrap();
        assert_eq!(host_tab.editor.read(cx).buffer.text(), host);
    });
}

#[gpui::test]
fn reading_instance_ids_preserve_host_names_and_separate_repeated_embeds(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("Host.md"), "Host").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        let host = ReadingContext::host(&this.tabs[this.active]);
        let mut first = host.clone();
        first.instance = 1;
        let mut second = first.clone();
        second.instance = 2;
        let legacy: gpui::ElementId = ("inline", 3_usize).into();
        assert_eq!(host.id("inline", 3), legacy);
        assert_ne!(first.id("inline", 3), host.id("inline", 3));
        assert_ne!(first.id("inline", 3), second.id("inline", 3));
        assert_eq!(host.selector("reading-task-3"), "reading-task-3");
        assert_eq!(first.selector("reading-task-3"), "embed-1-reading-task-3");
    });
}
