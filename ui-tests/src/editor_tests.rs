use super::{Editor, LINE};
use gpui::{EntityInputHandler, Modifiers, TestAppContext, point, px};

#[gpui::test]
fn control_click_follows_tags_in_source_and_live_preview_but_not_yaml_or_code(
    cx: &mut TestAppContext,
) {
    use std::{cell::RefCell, rc::Rc};
    let text = "---\n#metadata\n---\n\nLead\n\nA **bold** #work/research tag.\n\n`#literal`\n";
    let (view, cx) = cx.add_window_view(|window, cx| {
        let e = Editor::new(text, false, "", cx);
        e.focus.focus(window);
        e
    });
    let targets = Rc::new(RefCell::new(Vec::new()));
    let observed = targets.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &super::FollowLink, _| {
            observed.borrow_mut().push(event.0.clone())
        })
    });
    for live in [false, true] {
        view.update(cx, |e, cx| {
            e.set_live_preview(live, cx);
            e.jump_to(text.len(), cx);
        });
        cx.run_until_parked();
        for needle in ["#metadata", "#literal", "#work/research"] {
            let position = view.read_with(cx, |e, _| {
                e.point_for(text.find(needle).unwrap() + 2) + point(px(0.), px(LINE / 2.))
            });
            cx.simulate_click(
                position,
                Modifiers {
                    control: true,
                    ..Default::default()
                },
            );
        }
    }
    assert_eq!(
        *targets.borrow(),
        vec!["opalglass-tag:work/research", "opalglass-tag:work/research"]
    );
    view.read_with(cx, |e, _| assert_eq!(e.buffer.text(), text));
}

#[gpui::test]
fn list_and_formatting_keys_are_undoable_in_source_and_live_preview(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|w, cx| {
        let e = Editor::new("- [x] done", false, "", cx);
        e.focus.focus(w);
        e
    });
    cx.simulate_keystrokes("ctrl-end enter");
    view.read_with(cx, |e, _| assert_eq!(e.buffer.text(), "- [x] done\n- [ ] "));
    cx.simulate_keystrokes("tab");
    view.read_with(cx, |e, _| {
        assert_eq!(e.buffer.text(), "- [x] done\n    - [ ] ")
    });
    cx.simulate_keystrokes("shift-tab enter");
    view.read_with(cx, |e, _| assert_eq!(e.buffer.text(), "- [x] done\n"));
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |e, _| assert_eq!(e.buffer.text(), "- [x] done\n- [ ] "));
    view.update(cx, |e, cx| {
        e.set_text("éclair", cx);
        e.set_live_preview(true, cx);
    });
    cx.simulate_keystrokes("ctrl-a ctrl-alt-b");
    view.read_with(cx, |e, _| assert_eq!(e.buffer.text(), "**éclair**"));
    cx.simulate_keystrokes("ctrl-z ctrl-alt-i");
    view.read_with(cx, |e, _| assert_eq!(e.buffer.text(), "*éclair*"));
    cx.simulate_keystrokes("ctrl-z ctrl-alt-c");
    view.read_with(cx, |e, _| assert_eq!(e.buffer.text(), "`éclair`"));
    cx.simulate_keystrokes("ctrl-z ctrl-enter");
    view.read_with(cx, |e, _| assert_eq!(e.buffer.text(), "- [ ] éclair"));
}

#[gpui::test]
fn colored_highlight_clicks_preserve_delimiters_and_unicode(cx: &mut TestAppContext) {
    let text = "Lead\n\n==🔴éclair 👩‍💻== and ==**bold**==\n";
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut editor = Editor::new(text, false, "", cx);
        editor.set_live_preview(true, cx);
        editor.focus.focus(window);
        editor
    });
    cx.run_until_parked();
    let position = view.read_with(cx, |editor, _| {
        let row = editor
            .visible_rows
            .iter()
            .find(|r| r.projection.text.starts_with("éclair"))
            .unwrap();
        assert!(
            row.projection
                .spans
                .iter()
                .any(|s| s.style.highlight == Some(opalglass::markdown::HighlightColor::Red))
        );
        row.origin + point(px(0.), row.line_height / 2.)
    });
    cx.simulate_click(position, Modifiers::default());
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.cursor(), text.find('é').unwrap())
    });
    cx.simulate_input("X");
    view.read_with(cx, |editor, _| {
        assert!(editor.buffer.text().contains("==🔴Xéclair 👩‍💻=="))
    });
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), text));
}

#[gpui::test]
fn live_preview_clicks_edit_source_and_preserve_ime_and_history(cx: &mut TestAppContext) {
    let text = "Lead\n# Heading\n\n**bold** and *italic* and `code`\n[[Target|éclair 👩‍💻]]\n- [x] task\n```rs\n**literal**\n```\n";
    let alias = text.find("éclair").unwrap();
    let code = text.find("```rs").unwrap()..text.len();
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut editor = Editor::new(text, false, "", cx);
        editor.set_code_blocks(vec![code]);
        editor.set_live_preview(true, cx);
        editor.focus.focus(window);
        editor
    });
    cx.simulate_resize(gpui::size(px(1100.), px(900.)));
    cx.run_until_parked();
    let position = view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), text);
        let heading = editor
            .visible_rows
            .iter()
            .find(|r| r.range.start == text.find("# Heading").unwrap())
            .unwrap();
        assert_eq!(heading.projection.text, "Heading");
        assert!(heading.line_height > px(LINE));
        let formatted = editor
            .visible_rows
            .iter()
            .find(|r| r.range.start == text.find("**bold**").unwrap())
            .unwrap();
        assert_eq!(formatted.projection.text, "bold and italic and code");
        assert!(formatted.projection.spans.iter().any(|s| s.style.bold));
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|r| r.projection.text == "**literal**")
        );
        let link = editor
            .visible_rows
            .iter()
            .find(|r| r.projection.text == "éclair 👩‍💻")
            .unwrap();
        link.origin + point(px(0.), link.line_height / 2.)
    });
    cx.simulate_click(position, Modifiers::default());
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.cursor(), alias);
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|r| r.projection.text == "[[Target|éclair 👩‍💻]]")
        );
    });
    cx.simulate_input("X");
    view.read_with(cx, |editor, _| {
        assert!(editor.buffer.text().contains("[[Target|Xéclair"))
    });
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), text));
    view.update_in(cx, |editor, window, cx| {
        let emoji = editor.buffer.text().find("👩‍💻").unwrap();
        let range = editor.buffer.to_utf16(emoji..emoji + "👩‍💻".len());
        editor.replace_and_mark_text_in_range(Some(range), "ni", Some(0..2), window, cx);
        editor.replace_text_in_range(None, "你", window, cx);
    });
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), text));
    cx.simulate_keystrokes("ctrl-a ctrl-c");
    view.read_with(cx, |editor, cx| {
        assert_eq!(editor.buffer.selected(), text);
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), text);
    });
    view.update(cx, |editor, cx| editor.set_live_preview(false, cx));
    view.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), text));
}

#[gpui::test]
fn live_preview_keyboard_navigation_and_virtualization(cx: &mut TestAppContext) {
    let text = format!(
        "Top\n# Heading\n**bold** and [[Note|alias]]\n{}",
        "A **formatted** paragraph 👩‍💻\n\n".repeat(20_000)
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut editor = Editor::new(&text, false, "", cx);
        editor.set_live_preview(true, cx);
        editor.focus.focus(window);
        editor
    });
    cx.simulate_keystrokes("down");
    view.read_with(cx, |editor, _| assert_eq!(editor.buffer.line_column().0, 2));
    cx.simulate_keystrokes("up");
    view.read_with(cx, |editor, _| assert_eq!(editor.buffer.line_column().0, 1));
    cx.simulate_keystrokes("down down");
    view.read_with(cx, |editor, _| assert_eq!(editor.buffer.line_column().0, 3));
    cx.simulate_keystrokes("ctrl-end shift-pageup");
    view.read_with(cx, |editor, _| {
        assert!(editor.buffer.line_column().0 > 39_000);
        assert!(editor.first_paint_indexed_rows.unwrap() <= 128);
        assert!(editor.cached_rows.len() <= 256);
        assert!(editor.live_lines.len() <= 128);
        assert!(editor.visible_rows.len() < 100);
        let caret = editor.point_for(editor.buffer.cursor());
        assert!(caret.y >= editor.bounds.top() && caret.y < editor.bounds.bottom());
        assert_eq!(editor.buffer.text(), text);
    });
}

#[gpui::test]
fn word_shortcuts_and_page_selection(cx: &mut TestAppContext) {
    let text = format!(
        "café e\u{301}lan\n{}",
        "A longer line of text\n".repeat(200)
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        let editor = Editor::new(&text, false, "", cx);
        editor.focus.focus(window);
        editor
    });
    cx.simulate_keystrokes("ctrl-right ctrl-shift-right");
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.selected(), " e\u{301}lan")
    });
    cx.simulate_keystrokes("ctrl-backspace ctrl-z");
    view.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), text));
    cx.simulate_keystrokes("ctrl-home shift-pagedown");
    view.read_with(cx, |editor, _| {
        assert!(editor.buffer.line_column().0 > 5);
        assert_eq!(editor.buffer.anchor(), 0);
        assert!(!editor.buffer.selected().is_empty());
        let caret = editor.point_for(editor.buffer.cursor());
        assert!(caret.y >= editor.bounds.top() && caret.y < editor.bounds.bottom());
    });
    cx.simulate_keystrokes("pageup");
    view.read_with(cx, |editor, _| assert_eq!(editor.buffer.cursor(), 0));
}

#[gpui::test]
fn vertical_movement_retains_column_across_short_lines(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let editor = Editor::new("abcdefghij\nx\nabcdefghij", false, "", cx);
        editor.focus.focus(window);
        editor
    });
    cx.simulate_keystrokes("right right right right right down down");
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.line_column(), (3, 6))
    });
    cx.simulate_keystrokes("up up");
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.line_column(), (1, 6))
    });
}

#[gpui::test]
fn large_source_shapes_only_visible_rows_and_navigates_beyond_viewport(cx: &mut TestAppContext) {
    let text = "line 😀\n".repeat(20_000);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let editor = Editor::new(&text, false, "", cx);
        editor.focus.focus(window);
        editor
    });
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.first_paint_indexed_rows, Some(128));
        assert!(editor.source_complete);
        assert!(editor.source_ranges.len() > 20_000);
        assert!(editor.cached_rows.len() < 100);
    });
    cx.simulate_keystrokes(&vec!["down"; 60].join(" "));
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.line_column().0, 61)
    });
    cx.simulate_keystrokes("ctrl-end");
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.cursor(), text.len());
        let point = editor.point_for(text.len());
        assert!(point.y >= editor.bounds.top() && point.y < editor.bounds.bottom());
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|r| r.range.end == text.len())
        );
        assert!(editor.cached_rows.len() <= 256);
    });
    cx.simulate_input("ş");
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), text));
}

#[gpui::test]
fn opening_source_index_preserves_edits_and_immediate_end_navigation(cx: &mut TestAppContext) {
    use gpui::AppContext;
    let text = "line 😀\n".repeat(20_000);
    let editor = cx.new(|cx| Editor::new(&text, false, "", cx));
    editor.update(cx, |editor, cx| {
        editor.prepare_source();
        assert!(!editor.source_complete);
        assert_eq!(editor.source_ranges.len(), 128);
        editor.start_source_index(editor.buffer.revision(), cx);
        // An edit races the pending snapshot; it must never install old ranges.
        editor.buffer.replace(0..0, "inserted\n");
    });
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        assert!(!editor.source_complete);
        editor.prepare_source();
        assert_eq!(
            editor.source_ranges,
            opalglass::document::source_rows(editor.buffer.text())
        );
        assert!(editor.buffer.text().starts_with("inserted\n"));
        editor.set_text(&text, cx);
        editor.prepare_source();
        assert!(!editor.source_complete);
        // Jump before the first frame/background task gets a chance to run.
        editor.buffer.move_to(text.len(), false);
        editor.prepare_source();
        assert!(editor.source_complete);
        assert_eq!(editor.source_ranges.last().unwrap().end, text.len());
    });
}

#[gpui::test]
fn long_physical_line_keeps_caret_and_unicode_selection_visible(cx: &mut TestAppContext) {
    let text = "a😀e\u{301} ".repeat(4000);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let editor = Editor::new(&text, false, "", cx);
        editor.focus.focus(window);
        editor
    });
    cx.simulate_keystrokes("ctrl-end shift-left shift-left");
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.selected(), "e\u{301} ");
        let point = editor.point_for(editor.buffer.cursor());
        assert!(point.y >= editor.bounds.top() && point.y < editor.bounds.bottom());
        assert!(editor.cached_rows.iter().all(|r| r.range.len() < 300));
    });
}

#[gpui::test]
fn mouse_selection_and_utf16_composition(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let editor = Editor::new("Alpha 😀 beta", false, "", cx);
        editor.focus.focus(window);
        editor
    });
    let position = view.read_with(cx, |editor, _| {
        assert!(!editor.visible_rows.is_empty());
        editor.point_for(6) + point(px(0.), px(LINE / 2.))
    });
    cx.simulate_click(position, Modifiers::default());
    cx.simulate_keystrokes("shift-right");
    view.read_with(cx, |editor, _| assert_eq!(editor.buffer.selected(), "😀"));
    view.update_in(cx, |editor, window, cx| {
        // UTF-16 positions 6..8 cover one emoji, not two UTF-8 bytes.
        editor.replace_and_mark_text_in_range(Some(6..8), "ni", Some(0..2), window, cx);
        assert_eq!(editor.buffer.text(), "Alpha ni beta");
        assert_eq!(editor.marked_text_range(window, cx), Some(6..8));
        editor.replace_text_in_range(None, "你", window, cx);
        assert_eq!(editor.buffer.text(), "Alpha 你 beta");
    });
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), "Alpha 😀 beta")
    });
}

#[gpui::test]
fn long_single_line_field_keeps_caret_visible(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let editor = Editor::new("", true, "", cx);
        editor.focus.focus(window);
        editor
    });
    let long = "a/".repeat(200);
    // Paste is intentionally one transaction.
    view.update_in(cx, |editor, window, cx| {
        editor.replace_text_in_range(None, &long, window, cx)
    });
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), long);
        assert!(editor.scroll_x > px(0.));
        assert!(editor.point_for(editor.buffer.cursor()).x < editor.bounds.right());
    });
    cx.simulate_keystrokes("home");
    view.read_with(cx, |editor, _| assert_eq!(editor.scroll_x, px(0.)));
}
