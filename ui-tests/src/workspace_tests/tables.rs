use super::{TestAppContext, ViewMode, Workspace};
use gpui::{Modifiers, point, px, size};

#[gpui::test]
fn huge_table_cells_render_in_bounded_rows_and_keep_complete_selection_and_links(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    let label = format!("{}FINISH", "é 😀 e\u{301} words ".repeat(12_000));
    let source =
        format!("| Long | Short |\n| --- | --- |\n| [[Target\\|{label}]] | short |\n\n# End\n");
    std::fs::write(folder.path().join("A.md"), &source).unwrap();
    std::fs::write(folder.path().join("Target.md"), "# Target\n").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.simulate_resize(size(px(1200.), px(800.)));
    cx.run_until_parked();
    let body_rows = view.read_with(cx, |this, _| {
        let tab = &this.tabs[this.active];
        assert!(tab.prepared.complete);
        let rows: Vec<_> = tab
            .prepared
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.table.as_ref().is_some_and(|t| !t.row.header))
            .map(|(index, _)| index)
            .collect();
        assert!(rows.len() > 100);
        assert_eq!(
            rows.iter()
                .map(|index| tab.prepared.rows[*index].table.as_ref().unwrap().text(0))
                .collect::<String>(),
            label
        );
        rows
    });
    for mode in [ViewMode::Preview, ViewMode::Split] {
        view.update_in(cx, |this, w, cx| this.set_mode(mode, w, cx));
        for index in [
            body_rows[0],
            body_rows[body_rows.len() / 2],
            *body_rows.last().unwrap(),
        ] {
            view.update(cx, |this, cx| {
                let tab = &this.tabs[this.active];
                tab.preview_scroll.scroll_to(gpui::ListOffset {
                    item_ix: tab.preview_projection.list_index(index),
                    offset_in_item: px(0.),
                });
                this.preview_pane.update(cx, |_, cx| cx.notify());
            });
            cx.run_until_parked();
            let cell = cx
                .debug_bounds(Box::leak(
                    format!("reading-table-{index}-0").into_boxed_str(),
                ))
                .unwrap_or_else(|| {
                    panic!(
                        "table slice {index} missing (split={})",
                        mode == ViewMode::Split
                    )
                });
            assert!(
                cell.size.height < px(2000.),
                "one slice laid out the full cell: {cell:?}"
            );
            view.read_with(cx, |this, _| {
                let chunk = this.tabs[this.active].prepared.rows[index]
                    .table
                    .as_ref()
                    .unwrap();
                assert!(chunk.text(0).len() <= opalglass::tables::MAX_CELL_BYTES);
                assert_eq!(chunk.row.cells[0].text.as_ref(), label);
                if index == *body_rows.last().unwrap() {
                    assert!(chunk.text(0).ends_with("FINISH"));
                }
            });
        }
    }
    // The source selection remains the complete Markdown in either editing mode.
    view.update_in(cx, |this, w, cx| this.set_mode(ViewMode::Source, w, cx));
    for live in [false, true] {
        view.update_in(cx, |this, w, cx| {
            this.tabs[this.active].editor.update(cx, |editor, cx| {
                editor.set_live_preview(live, cx);
                editor.focus.focus(w);
            });
        });
        cx.simulate_keystrokes("ctrl-a ctrl-c");
        cx.update(|_, cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), source));
    }
    // A link remains clickable in a continuation far beyond the first slice.
    let middle = body_rows[body_rows.len() / 2];
    view.update_in(cx, |this, w, cx| {
        this.set_mode(ViewMode::Preview, w, cx);
        let tab = &this.tabs[this.active];
        tab.preview_scroll.scroll_to(gpui::ListOffset {
            item_ix: tab.preview_projection.list_index(middle),
            offset_in_item: px(0.),
        });
        this.preview_pane.update(cx, |_, cx| cx.notify());
    });
    cx.run_until_parked();
    // Refresh test selectors after the pane moved from Split to Reading.
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let cell = cx
        .debug_bounds(Box::leak(
            format!("reading-table-{middle}-0").into_boxed_str(),
        ))
        .unwrap();
    let position = cell.origin + point(px(28.), px(14.));
    cx.simulate_event(gpui::MouseMoveEvent {
        position,
        ..Default::default()
    });
    cx.simulate_click(position, Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert_eq!(
            this.tabs[this.active].path.to_str(),
            Some("Target.md"),
            "status={}",
            this.status
        )
    });
    assert_eq!(
        std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
        source
    );
}
