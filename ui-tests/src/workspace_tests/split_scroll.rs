use super::{TestAppContext, Workspace, click};

#[gpui::test]
fn split_scroll_lock_follows_source_and_unlocks(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let text = (0..300).map(|i| format!("# Section {i}\n\nA **formatted** paragraph with [[Alias|short text]].\n\n\n> A quotation\n\n- item\n\n")).collect::<String>();
    std::fs::write(folder.path().join("A.md"), &text).unwrap();
    std::fs::write(folder.path().join("B.md"), &text).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(800.)));
    cx.run_until_parked();
    let wheel = |cx: &mut gpui::VisualTestContext, pane, delta| {
        let position = cx.debug_bounds(pane).unwrap().center();
        cx.simulate_event(gpui::ScrollWheelEvent {
            position,
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(delta))),
            ..Default::default()
        });
        cx.run_until_parked();
    };
    let assert_aligned = |cx: &gpui::VisualTestContext| {
        view.read_with(cx, |this, cx| {
            let tab = &this.tabs[this.active];
            let (_, byte) = tab.editor.read(cx).scroll_anchor.unwrap();
            let expected = tab
                .prepared
                .rows
                .partition_point(|row| row.source.start as f64 <= byte)
                .saturating_sub(1);
            let actual = tab.preview_scroll.logical_scroll_top();
            // A blank source line can map exactly to the end of a preview row.
            assert!(
                actual.item_ix.abs_diff(expected) <= 1,
                "expected {expected}, got {} at {byte}",
                actual.item_ix
            );
        });
    };
    wheel(cx, "source-pane", -700.);
    view.read_with(cx, |this, _| assert!(this.scroll_locked));
    assert_aligned(cx);
    let sidebar_before = view.read_with(cx, |this, cx| this.sidebar.read(cx).render_count);
    for _ in 0..5 {
        wheel(cx, "source-pane", -95.);
        assert_aligned(cx);
    }
    view.read_with(cx, |this, cx| {
        assert_eq!(this.sidebar.read(cx).render_count, sidebar_before)
    });
    let before = view.read_with(cx, |this, cx| {
        this.tabs[this.active]
            .editor
            .read(cx)
            .scroll_anchor
            .unwrap()
            .1
    });
    wheel(cx, "preview-pane", -170.);
    let after = view.read_with(cx, |this, cx| {
        this.tabs[this.active]
            .editor
            .read(cx)
            .scroll_anchor
            .unwrap()
            .1
    });
    assert!(
        after > before,
        "locked reading wheel should scroll the editor too: {before} -> {after}"
    );
    assert_aligned(cx);
    // Keyboard navigation, reflow, and switching tabs must resynchronize too.
    view.update(cx, |this, cx| {
        this.tabs[this.active].editor.update(cx, |editor, cx| {
            let at = editor.buffer.text().find("# Section 90").unwrap();
            editor.jump_to(at, cx);
        })
    });
    cx.run_until_parked();
    assert_aligned(cx);
    cx.simulate_resize(gpui::size(gpui::px(950.), gpui::px(700.)));
    cx.run_until_parked();
    assert_aligned(cx);
    view.update_in(cx, |this, w, cx| this.open_note("B.md".into(), w, cx));
    cx.run_until_parked();
    assert_aligned(cx);
    wheel(cx, "source-pane", -600.);
    assert_aligned(cx);
    click(cx, "source");
    wheel(cx, "source-pane", -400.);
    click(cx, "split");
    cx.run_until_parked();
    assert_aligned(cx);
    // Rebuilding the preview after edits must retain source mapping.
    view.update(cx, |this, cx| {
        this.tabs[this.active].editor.update(cx, |editor, cx| {
            editor.buffer.replace(0..0, "# Inserted\n\n");
            editor.jump_to(0, cx);
        })
    });
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(75));
    cx.run_until_parked();
    wheel(cx, "source-pane", -600.);
    assert_aligned(cx);
    click(cx, "lock-reading-scroll");
    // An explicit unlock survives leaving and returning to split view.
    click(cx, "source");
    click(cx, "split");
    cx.run_until_parked();
    view.read_with(cx, |this, _| assert!(!this.scroll_locked));
    let before = view.read_with(cx, |this, _| {
        this.tabs[this.active].preview_scroll.logical_scroll_top()
    });
    wheel(cx, "source-pane", -250.);
    view.read_with(cx, |this, _| {
        let after = this.tabs[this.active].preview_scroll.logical_scroll_top();
        assert_eq!(after.item_ix, before.item_ix);
        assert_eq!(after.offset_in_item, before.offset_in_item);
    });
    let before = view.read_with(cx, |this, cx| {
        this.tabs[this.active].editor.read(cx).scroll_anchor
    });
    wheel(cx, "preview-pane", -250.);
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[this.active].editor.read(cx).scroll_anchor, before)
    });
    click(cx, "lock-reading-scroll");
    view.update(cx, |this, cx| {
        this.tabs[this.active].editor.update(cx, |editor, cx| {
            editor.jump_to(editor.buffer.text().len(), cx);
        })
    });
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        let tab = &this.tabs[this.active];
        assert!(
            tab.preview_scroll
                .bounds_for_item(tab.prepared.rows.len() - 1)
                .is_some()
        );
    });
}
