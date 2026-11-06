use super::{Editor, PAD};
use gpui::{TestAppContext, point, px, size};

#[gpui::test]
fn line_numbers_keep_padding_across_digit_counts_wrapping_and_font_changes(
    cx: &mut TestAppContext,
) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let editor = Editor::new("", false, "", cx);
        editor.focus.focus(window);
        editor
    });
    cx.simulate_resize(size(px(420.), px(600.)));
    for count in [9, 10, 100, 100_000] {
        let text = format!(
            "{}a long final line with é and words that wrap. {}",
            "row\n".repeat(count - 1),
            "wrapped words ".repeat(8)
        );
        view.update(cx, |editor, cx| {
            editor.set_text(&text, cx);
            editor.jump_to(text.len(), cx);
        });
        cx.run_until_parked();
        for font_size in [10., 28.] {
            view.update(cx, |editor, cx| editor.set_preferences(font_size, true, cx));
            cx.run_until_parked();
            // Preferences preserve the scroll anchor, so reveal the target again
            // after reflow before inspecting its actual painted geometry.
            view.update(cx, |editor, cx| editor.reveal_cursor(cx));
            cx.run_until_parked();
            view.read_with(cx, |editor, _| {
                let row = editor
                    .visible_rows
                    .iter()
                    .find(|r| r.range.end == text.len())
                    .unwrap_or_else(|| panic!(
                        "last row not painted: count={count} font={font_size} visible={:?} scroll={:?} source_rows={}",
                        editor.visible_rows.iter().map(|row| row.range.clone()).collect::<Vec<_>>(),
                        editor.source_list.logical_scroll_top(),
                        editor.source_ranges.len(),
                    ));
                let number = row.number_bounds.expect("physical line number is painted");
                assert!(
                    number.left() >= editor.bounds.left() + px(PAD),
                    "{count} lines: {number:?}"
                );
                assert!(
                    row.origin.x - number.right() >= px(24.),
                    "line-number gap must leave 24 px before text, got {:?}",
                    row.origin.x - number.right()
                );
                assert!(
                    row.line.size(row.line_height).height > row.line_height,
                    "last row wraps"
                );
                let at = row.range.start + "a long final line with ".len();
                let point = editor.point_for(at) + point(px(0.), row.line_height / 2.);
                assert_eq!(
                    editor.index_for(point),
                    at,
                    "caret hit-testing survives the gutter"
                );
                assert_eq!(editor.buffer.text(), text);
            });
        }
    }
    view.update(cx, |editor, cx| editor.set_preferences(14., false, cx));
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert!(
            editor
                .visible_rows
                .iter()
                .all(|row| row.number_bounds.is_none())
        );
        assert_eq!(editor.gutter(), PAD);
    });
}
