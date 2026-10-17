use super::*;

const HOST: &str = "# Host\n\n![[Source#Part]]\n";

fn source() -> String {
    format!(
        "# Before\n\n{}Outside é[^n].\n\n# Part\n\nInside[^n].\n\n# End\n\n[^n]: Shared footer\n",
        "Padding paragraph.\n\n".repeat(500)
    )
}

fn fixture() -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("Host.md"), HOST).unwrap();
    std::fs::write(folder.path().join("Source.md"), source()).unwrap();
    folder
}

fn return_widget(view: &Entity<Workspace>, cx: &mut VisualTestContext) -> (u64, usize, String) {
    let id = instances(view, cx, "Source#Part")[0].id;
    show_instance(view, cx, id);
    let document = document(view, cx, id);
    let definition = &document.prepared.footnotes.definitions[0];
    let outside = definition.references[0];
    let last = definition.sections.last().unwrap().end - 1;
    assert!(outside < document.source.find("# Part").unwrap());
    show_row(view, cx, id, last);
    (id, outside, format!("embed-{id}-footnote-return-{last}-0"))
}

#[gpui::test]
fn embedded_return_outside_the_section_waits_for_source_opening_and_analysis(
    cx: &mut TestAppContext,
) {
    let folder = fixture();
    let (view, cx) = reading(cx, folder.path());
    let (_, outside, selector) = return_widget(&view, cx);
    assert!(outside > 8 * 1024);
    view.read_with(cx, |this, _| assert_eq!(this.tabs.len(), 1));
    click_selector(cx, selector);
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        assert_eq!(tab.path, Path::new("Source.md"));
        assert_eq!(tab.editor.read(cx).buffer.cursor(), outside);
        assert!(tab.prepared.analysis_complete());
        assert!(tab.pending_preview.is_none());
        let row = tab.prepared.row_at_source(outside);
        let item = tab.preview_projection.list_index(row);
        assert!(tab.preview_scroll.bounds_for_item(item).is_some());
        let host = this
            .tabs
            .iter()
            .find(|tab| tab.path == Path::new("Host.md"))
            .unwrap();
        assert_eq!(host.editor.read(cx).buffer.text(), HOST);
    });
}

#[gpui::test]
fn embedded_return_does_not_apply_old_offsets_to_an_externally_changed_source(
    cx: &mut TestAppContext,
) {
    let folder = fixture();
    let (view, cx) = reading(cx, folder.path());
    let (_, outside, selector) = return_widget(&view, cx);
    let changed = format!("New external paragraph.\n\n{}", source());
    std::fs::write(folder.path().join("Source.md"), &changed).unwrap();
    // Keep the old visible widget, then let its fallback read the changed file.
    click_selector(cx, selector);
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        assert_eq!(tab.path, Path::new("Source.md"));
        assert_eq!(tab.editor.read(cx).buffer.text(), changed);
        assert_eq!(tab.editor.read(cx).buffer.cursor(), 0);
        assert_ne!(tab.editor.read(cx).buffer.cursor(), outside);
        assert!(tab.pending_preview.is_none());
    });
}

#[gpui::test]
fn newer_navigation_cancels_the_embedded_return_destination(cx: &mut TestAppContext) {
    let folder = fixture();
    let (view, cx) = reading(cx, folder.path());
    let (id, outside, _) = return_widget(&view, cx);
    view.update_in(cx, |this, window, cx| {
        let context = this.reading_context_for(id, cx).unwrap();
        this.follow_reading(
            context,
            format!("{}{outside}", opalglass::footnotes::BACK_PREFIX),
            window,
            cx,
        );
        assert!(this.note_loading);
        this.open_note("Host.md".into(), window, cx);
    });
    settle(cx);
    view.read_with(cx, |this, cx| {
        assert!(!this.note_loading);
        assert_eq!(this.tabs.len(), 1);
        let tab = &this.tabs[this.active];
        assert_eq!(tab.path, Path::new("Host.md"));
        assert_eq!(tab.editor.read(cx).buffer.cursor(), 0);
        assert!(tab.pending_preview.is_none());
    });
}
