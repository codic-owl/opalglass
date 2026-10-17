use super::Editor;
use gpui::{ListOffset, TestAppContext, px};
use opalglass::prepared::Prepared;

fn physical_row(editor: &Editor, byte: usize) -> usize {
    editor
        .source_ranges
        .partition_point(|range| range.start <= byte)
        .saturating_sub(1)
}

#[gpui::test]
fn hidden_syntax_has_no_list_rows_and_revealing_then_leaving_compacts_it_again(
    cx: &mut TestAppContext,
) {
    let text = "Lead\n\n%% begin\nhidden one\nhidden two\nend %%\n\n[ref]: Target.md\n\n[^unused]: Hidden footnote\n\n    Hidden continuation\n\nTail\n";
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut editor = Editor::new(text, false, "", cx);
        editor.set_live_context(Prepared::new(text).live, editor.buffer.revision());
        editor.set_live_preview(true, cx);
        editor.jump_to(text.len(), cx);
        editor.focus.focus(window);
        editor
    });
    cx.run_until_parked();
    let compact_count = view.read_with(cx, |editor, _| {
        for hidden in [
            "%% begin",
            "hidden one",
            "hidden two",
            "end %%",
            "[ref]:",
            "[^unused]:",
            "    Hidden continuation",
        ] {
            let row = physical_row(editor, text.find(hidden).unwrap());
            assert!(!editor.visibility.rows.is_visible(row), "{hidden}");
        }
        assert!(
            editor
                .visible_rows
                .iter()
                .all(|row| !row.projection.text.contains("hidden one")
                    && !row.projection.text.contains("Hidden footnote"))
        );
        assert!(editor.visibility.rows.len() + 6 < editor.source_ranges.len());
        assert_eq!(
            editor.source_list.item_count(),
            editor.visibility.rows.len()
        );
        editor.visibility.rows.len()
    });
    for (target, raw_fragments) in [
        (
            "hidden one",
            vec!["%% begin", "hidden one", "hidden two", "end %%"],
        ),
        ("[ref]:", vec!["[ref]: Target.md"]),
        (
            "Hidden continuation",
            vec!["[^unused]: Hidden footnote", "    Hidden continuation"],
        ),
    ] {
        view.update(cx, |editor, cx| {
            editor.jump_to(text.find(target).unwrap(), cx)
        });
        cx.run_until_parked();
        view.read_with(cx, |editor, _| {
            for raw in &raw_fragments {
                let row = physical_row(editor, text.find(raw).unwrap());
                assert!(editor.visibility.rows.is_visible(row), "{raw}");
                assert!(
                    editor
                        .visible_rows
                        .iter()
                        .any(|row| row.projection.text == *raw),
                    "{raw}"
                );
            }
            assert!(editor.visibility.rows.len() > compact_count);
            assert_eq!(editor.buffer.text(), text);
        });
        view.update(cx, |editor, cx| editor.jump_to(text.len(), cx));
        cx.run_until_parked();
        view.read_with(cx, |editor, _| {
            assert_eq!(editor.visibility.rows.len(), compact_count);
            assert_eq!(editor.source_list.item_count(), compact_count);
        });
    }
}

#[gpui::test]
fn physical_scroll_survives_many_hidden_lines_mode_switches_and_saved_position_restore(
    cx: &mut TestAppContext,
) {
    let text = (0..100)
        .map(|index| {
            format!("Visible row {index:03}\n%% begin {index}\nhidden contents {index}\nend %%\n\n")
        })
        .collect::<String>();
    let target_byte = text.find("Visible row 060").unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut editor = Editor::new(&text, false, "", cx);
        editor.set_live_context(Prepared::new(&text).live, editor.buffer.revision());
        editor.set_live_preview(true, cx);
        editor.focus.focus(window);
        editor
    });
    cx.run_until_parked();
    let physical = view.update(cx, |editor, cx| {
        assert!(editor.source_complete);
        let physical = physical_row(editor, target_byte);
        let visible = editor.visibility.rows.list_index(physical);
        assert!(visible < physical / 2);
        editor.source_list.scroll_to(ListOffset {
            item_ix: visible,
            offset_in_item: px(3.),
        });
        editor.reveal = false;
        cx.notify();
        physical
    });
    cx.run_until_parked();
    let saved = view.read_with(cx, |editor, _| {
        let saved = editor.saved_scroll();
        assert_eq!(saved.row, physical);
        assert!((saved.offset - 3.).abs() < 0.1);
        assert_eq!(
            editor
                .visibility
                .rows
                .row(editor.source_list.logical_scroll_top().item_ix),
            physical
        );
        let (_, anchor) = editor.scroll_anchor.unwrap();
        assert!(anchor >= target_byte as f64);
        assert!(anchor <= editor.source_ranges[physical].end as f64);
        saved
    });
    for live in [false, true] {
        view.update(cx, |editor, cx| editor.set_live_preview(live, cx));
        cx.run_until_parked();
        view.read_with(cx, |editor, _| {
            assert_eq!(editor.saved_scroll().row, physical, "live={live}");
            assert!((editor.saved_scroll().offset - saved.offset).abs() < 0.1);
            assert_eq!(
                editor.visibility.rows.len(),
                if live {
                    editor.source_list.item_count()
                } else {
                    editor.source_ranges.len()
                }
            );
        });
    }
    view.update(cx, |editor, cx| {
        editor.source_list.scroll_to(ListOffset::default());
        editor.reveal = false;
        cx.notify();
    });
    cx.run_until_parked();
    view.update(cx, |editor, cx| editor.restore_position(0, 0, saved, cx));
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.saved_scroll().row, physical);
        assert!((editor.saved_scroll().offset - saved.offset).abs() < 0.1);
        assert_eq!(editor.buffer.cursor(), 0);
        assert_eq!(editor.buffer.text(), text);
        assert_eq!(
            editor
                .visibility
                .rows
                .row(editor.source_list.logical_scroll_top().item_ix),
            physical
        );
    });
}
