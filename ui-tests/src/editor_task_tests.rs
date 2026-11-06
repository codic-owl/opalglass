use super::Editor;
use gpui::{Modifiers, TestAppContext, point, px};

#[gpui::test]
fn live_preview_tasks_show_checkboxes_and_support_keyboard_toggle_and_undo(
    cx: &mut TestAppContext,
) {
    let source = "Lead\n\n- [ ] Todo\n- [x] Done\n\n[ ] Bare brackets\n";
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut editor = Editor::new(source, false, "", cx);
        editor.set_live_preview(true, cx);
        editor.focus.focus(window);
        editor
    });
    cx.run_until_parked();
    let todo = view.read_with(cx, |editor, _| {
        for (needle, expected) in [
            ("- [ ] Todo", "☐  Todo"),
            ("- [x] Done", "☑  Done"),
            ("[ ] Bare brackets", "[ ] Bare brackets"),
        ] {
            let start = source.find(needle).unwrap();
            let row = editor
                .visible_rows
                .iter()
                .find(|row| row.range.start == start)
                .unwrap();
            assert_eq!(row.projection.text.trim_end(), expected);
        }
        let row = editor
            .visible_rows
            .iter()
            .find(|row| row.range.start == source.find("- [ ] Todo").unwrap())
            .unwrap();
        row.origin + point(px(1.), row.line_height / 2.)
    });
    cx.simulate_click(todo, Modifiers::default());
    cx.simulate_keystrokes("ctrl-enter");
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), source.replacen("[ ]", "[x]", 1))
    });
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), source));
}
