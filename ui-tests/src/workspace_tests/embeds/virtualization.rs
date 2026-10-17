use super::*;
use opalglass::prepared::Prepared;

#[gpui::test]
fn large_embed_completion_preserves_the_host_anchor_and_virtualizes_the_tail(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    let host = format!(
        "# Host\n\n![[Huge]]\n\n# Below\n\n![[Huge]]\n\n{}",
        "Ordinary host paragraph.\n\n".repeat(80)
    );
    let huge = format!(
        "# Huge\n\n{}```txt\nHUGE TAIL é\n```\n",
        "A paragraph inside the large note.\n\n".repeat(5000)
    );
    std::fs::write(folder.path().join("Host.md"), &host).unwrap();
    std::fs::write(folder.path().join("Huge.md"), &huge).unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| {
        let workspace = Workspace::new(Some(folder.path().into()), window, cx);
        workspace.embedded_documents.update(cx, |cache, _| {
            // Control the actual cache completion so insertion happens while
            // the host heading below the first embed is at the viewport top.
            cache.test_loader(Arc::new(|_, _| Box::pin(std::future::pending())));
        });
        workspace
    });
    cx.run_until_parked();
    view.update_in(cx, |this, window, cx| {
        this.open_note("Host.md".into(), window, cx);
        this.set_mode(ViewMode::Preview, window, cx);
    });
    cx.simulate_resize(size(px(1200.), px(1000.)));
    settle(cx);
    let roots = instances(&view, cx, "Huge");
    assert_eq!(roots.len(), 2);
    let below = view.read_with(cx, |this, _| {
        let tab = &this.tabs[this.active];
        let physical = tab
            .prepared
            .rows
            .iter()
            .position(|row| row.text.as_ref() == "Below")
            .unwrap();
        let logical = tab.preview_projection.list_index(physical);
        (
            physical,
            tab.composition
                .projection
                .find_document_row(0, logical)
                .unwrap(),
        )
    });
    scroll_to(&view, cx, below.1);
    let before = view.read_with(cx, |this, _| {
        let tab = &this.tabs[this.active];
        let top = tab.preview_scroll.logical_scroll_top();
        (
            tab.composition.projection.row(top.item_ix).unwrap(),
            f32::from(top.offset_in_item),
        )
    });
    let input = view.read_with(cx, |this, _| {
        this.tabs[this.active].composition.instances[&roots[0].id]
            .input
            .clone()
            .unwrap()
    });
    let prepared = Arc::new(Prepared::new(&huge));
    let mut projection = opalglass::callouts::Projection::default();
    projection.rebuild(prepared.rows.len(), &prepared.callouts);
    projection.with_footnotes(prepared.rows.len(), &prepared.footnotes, &prepared.callouts);
    let loaded = Arc::new(Document {
        path: input.path.clone(),
        source: Arc::new(huge.clone()),
        prepared,
        token: input.token,
        projection,
        bytes: huge.len() * 4,
    });
    view.update_in(cx, |this, window, cx| {
        this.embedded_documents.update(cx, |cache, cx| {
            let serial = cache
                .test_serial(&input)
                .expect("visible repeated embed owns a pending request");
            cache.test_publish(&input, serial, Ok(loaded), window, cx);
        });
    });
    settle(cx);
    view.read_with(cx, |this, _| {
        let tab = &this.tabs[this.active];
        let top = tab.preview_scroll.logical_scroll_top();
        assert_eq!(
            tab.composition.projection.row(top.item_ix).unwrap(),
            before.0
        );
        assert!((f32::from(top.offset_in_item) - before.1).abs() < 1.);
        assert!(tab.composition.projection.len() > 10_000);
        let instance = &tab.composition.instances[&roots[1].id];
        let middle = instance.projection.list_index(2500);
        let item = tab
            .composition
            .projection
            .find_document_row(roots[1].id, middle)
            .unwrap();
        assert!(
            tab.preview_scroll.bounds_for_item(item).is_none(),
            "the large offscreen body must remain unpainted"
        );
    });
    assert!(
        cx.debug_bounds(Box::leak(
            format!("preview-column-{}", below.0).into_boxed_str()
        ))
        .is_some()
    );
    copy_code(&view, cx, roots[0].id, "HUGE TAIL é\n");
}

#[gpui::test]
fn later_repeated_embeds_load_after_the_recent_instance_limit(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let host = "![[Leaf]]\n\n".repeat(500);
    std::fs::write(folder.path().join("Host.md"), &host).unwrap();
    std::fs::write(folder.path().join("Leaf.md"), "```txt\nLEAF\n```\n").unwrap();
    let (view, cx) = reading(cx, folder.path());
    let repeated = instances(&view, cx, "Leaf");
    assert_eq!(repeated.len(), 500);
    for instance in repeated.iter().take(70) {
        show_instance(&view, cx, instance.id);
    }
    let last = repeated.last().unwrap().id;
    show_instance(&view, cx, last);
    copy_code(&view, cx, last, "LEAF\n");
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[this.active].editor.read(cx).buffer.text(), host);
        assert!(
            this.tabs[this.active].composition.instances[&last]
                .error
                .is_none()
        );
    });
}
