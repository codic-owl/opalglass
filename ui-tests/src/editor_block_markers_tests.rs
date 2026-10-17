use super::{Editor, LINE};
use gpui::{Modifiers, TestAppContext, point, px};

#[gpui::test]
fn block_markers_hide_until_editing_and_full_source_survives_mouse_copy_and_undo(
    cx: &mut TestAppContext,
) {
    let source = "# Top\n\nParagraph **é 👩‍💻** ^paragraph\n\nDuplicate **é** ^paragraph\n\n- [ ] Task ^task\n\n^list\n\nInline `^literal` and \\^escaped\n\nEnd\n";
    let prepared = opalglass::prepared::Prepared::new(source);
    let standalone = source.find("^list").unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut editor = Editor::new(source, false, "", cx);
        editor.set_live_context(prepared.live, editor.buffer.revision());
        editor.set_live_preview(true, cx);
        editor.jump_to(source.len(), cx);
        editor.focus.focus(window);
        editor
    });
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|row| row.projection.text == "Paragraph é 👩‍💻")
        );
        for marker in ["^paragraph", "^task", "^list"] {
            assert!(
                editor
                    .visible_rows
                    .iter()
                    .all(|row| !row.projection.text.contains(marker))
            );
        }
        assert!(
            editor
                .visible_rows
                .iter()
                .all(|row| !row.range.contains(&standalone))
        );
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|row| row.projection.text.contains("^literal"))
        );
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|row| row.projection.text.contains("^escaped"))
        );
        assert_eq!(editor.buffer.text(), source);
    });
    let at = source.find('é').unwrap();
    let position = view.read_with(cx, |editor, _| {
        editor.point_for(at) + point(px(0.), px(LINE / 2.))
    });
    cx.simulate_click(position, Modifiers::default());
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.cursor(), at);
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|row| row.projection.text.contains("^paragraph"))
        );
    });
    cx.simulate_input("X");
    view.read_with(cx, |editor, _| {
        assert!(editor.buffer.text().contains("**Xé 👩‍💻** ^paragraph"))
    });
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), source));
    cx.simulate_keystrokes("ctrl-a ctrl-c");
    cx.update(|_, cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), source));
    view.update(cx, |editor, cx| {
        editor.set_live_context(
            opalglass::prepared::Prepared::new(source).live,
            editor.buffer.revision(),
        );
        editor.jump_to(source.find("Duplicate").unwrap(), cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|row| row.projection.text == "Duplicate **é** ^paragraph")
        );
        assert_eq!(editor.buffer.text(), source);
    });
    view.update(cx, |editor, cx| {
        editor.jump_to(standalone, cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|row| row.projection.text == "^list")
        );
        assert_eq!(editor.buffer.text(), source);
    });
}
