use super::Editor;
use gpui::{Modifiers, TestAppContext, point, px};
use opalglass::prepared::Prepared;

const NESTED: &str = "Start\n\n> [!note]+ Outer **title**\n> Outer body\n>\n> > [!tip]- Inner **é**\n> > Inner body 👩‍💻\n>\n> Outer tail\n\nEnd\n";

fn source_row(editor: &Editor, byte: usize) -> usize {
    editor
        .source_ranges
        .partition_point(|range| range.start <= byte)
        .saturating_sub(1)
}

#[gpui::test]
fn nested_callout_buttons_fold_real_rows_and_preserve_independent_state(cx: &mut TestAppContext) {
    let context = Prepared::new(NESTED).live;
    let outer = context
        .callouts()
        .iter()
        .find(|live| live.callout.name == "note")
        .unwrap()
        .callout
        .source
        .start;
    let inner = context
        .callouts()
        .iter()
        .find(|live| live.callout.name == "tip")
        .unwrap()
        .callout
        .source
        .start;
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut editor = Editor::new(NESTED, false, "", cx);
        editor.set_live_context(context, editor.buffer.revision());
        editor.set_live_preview(true, cx);
        editor.focus.focus(window);
        editor
    });
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert!(!editor.collapsed(outer));
        assert!(editor.collapsed(inner));
        assert!(
            !editor
                .visibility
                .rows
                .is_visible(source_row(editor, NESTED.find("Inner body").unwrap()))
        );
    });
    let inner_selector = Box::leak(format!("live-callout-toggle-{inner}").into_boxed_str());
    let outer_selector = Box::leak(format!("live-callout-toggle-{outer}").into_boxed_str());
    let button = cx.debug_bounds(inner_selector).unwrap();
    cx.simulate_click(button.center(), Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert!(!editor.collapsed(inner));
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|row| row.projection.text == "Inner body 👩‍💻")
        );
    });
    let button = cx.debug_bounds(outer_selector).unwrap();
    cx.simulate_click(button.center(), Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert!(editor.collapsed(outer));
        assert!(!editor.collapsed(inner));
        let callout = editor
            .live_context
            .as_ref()
            .unwrap()
            .1
            .callouts()
            .iter()
            .find(|live| live.callout.source.start == outer)
            .unwrap();
        assert!(
            editor
                .visibility
                .rows
                .is_visible(source_row(editor, callout.title.start))
        );
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|row| row.range.start == callout.title.start)
        );
        for (index, range) in editor
            .source_ranges
            .iter()
            .enumerate()
            .filter(|(_, range)| {
                range.start > callout.title.end && range.start < callout.callout.source.end
            })
        {
            assert!(
                !editor.visibility.rows.is_visible(index),
                "folded source row {range:?}"
            );
            assert!(editor.visible_rows.iter().all(|row| row.range != *range));
        }
        assert_eq!(editor.buffer.text(), NESTED);
    });
    let button = cx.debug_bounds(outer_selector).unwrap();
    cx.simulate_click(button.center(), Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert!(!editor.collapsed(outer));
        assert!(!editor.collapsed(inner));
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|row| row.projection.text == "Inner body 👩‍💻")
        );
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|row| row.projection.text == "Outer tail")
        );
        assert_eq!(editor.buffer.text(), NESTED);
    });
}

#[gpui::test]
fn active_callout_title_and_unicode_body_reveal_source_and_keep_edit_undo_copy_exact(
    cx: &mut TestAppContext,
) {
    let text = "Start\n\n> [!tip]+ **é title**\n> Body **👩‍💻** and [link](Target.md).\n\nEnd\n";
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut editor = Editor::new(text, false, "", cx);
        editor.set_live_context(Prepared::new(text).live, editor.buffer.revision());
        editor.set_live_preview(true, cx);
        editor.jump_to(text.len(), cx);
        editor.focus.focus(window);
        editor
    });
    cx.run_until_parked();
    let title = text.find("é title").unwrap();
    let position = view.read_with(cx, |editor, _| {
        let row = editor
            .visible_rows
            .iter()
            .find(|row| row.projection.text == "é title")
            .unwrap();
        assert!(row.projection.spans.iter().any(|span| span.style.bold));
        editor.point_for(title) + point(px(0.), row.line_height / 2.)
    });
    cx.simulate_click(position, Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.cursor(), title);
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|row| row.projection.text == "> [!tip]+ **é title**")
        );
    });
    cx.simulate_input("ş");
    cx.simulate_keystrokes("ctrl-z");
    view.update(cx, |editor, cx| {
        assert_eq!(editor.buffer.text(), text);
        // Standalone editor tests install the same refreshed document context
        // that the workspace supplies asynchronously after a revision changes.
        editor.set_live_context(Prepared::new(text).live, editor.buffer.revision());
        editor.jump_to(text.len(), cx);
    });
    cx.run_until_parked();
    let emoji = text.find("👩‍💻").unwrap();
    let position = view.read_with(cx, |editor, _| {
        let row = editor
            .visible_rows
            .iter()
            .find(|row| row.projection.text.starts_with("Body "))
            .unwrap();
        editor.point_for(emoji) + point(px(0.), row.line_height / 2.)
    });
    cx.simulate_click(position, Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.cursor(), emoji);
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|row| row.projection.text == "> Body **👩‍💻** and [link](Target.md).")
        );
    });
    cx.simulate_keystrokes("shift-right ctrl-c");
    view.read_with(cx, |editor, _| assert_eq!(editor.buffer.selected(), "👩‍💻"));
    cx.update(|_, cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "👩‍💻"));
    cx.simulate_input("ş");
    cx.simulate_keystrokes("ctrl-z ctrl-a ctrl-c");
    view.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), text));
    cx.update(|_, cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), text));
}

#[gpui::test]
fn folding_the_active_body_moves_caret_to_title_and_explicit_navigation_opens_ancestors(
    cx: &mut TestAppContext,
) {
    let context = Prepared::new(NESTED).live;
    let outer = context
        .callouts()
        .iter()
        .find(|live| live.callout.name == "note")
        .unwrap();
    let (outer_id, outer_title) = (outer.callout.source.start, outer.title.clone());
    let inner_id = context
        .callouts()
        .iter()
        .find(|live| live.callout.name == "tip")
        .unwrap()
        .callout
        .source
        .start;
    let target = NESTED.find("Inner body").unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut editor = Editor::new(NESTED, false, "", cx);
        editor.set_live_context(context, editor.buffer.revision());
        editor.set_live_preview(true, cx);
        editor.jump_to(target, cx);
        editor.focus.focus(window);
        editor
    });
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert!(!editor.collapsed(outer_id));
        assert!(!editor.collapsed(inner_id));
    });
    let selector = Box::leak(format!("live-callout-toggle-{outer_id}").into_boxed_str());
    let button = cx.debug_bounds(selector).unwrap();
    cx.simulate_click(button.center(), Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert!(editor.collapsed(outer_id));
        assert!(
            outer_title.contains(&editor.buffer.cursor())
                || outer_title.end == editor.buffer.cursor()
        );
        assert!(
            !editor
                .visibility
                .rows
                .is_visible(source_row(editor, target))
        );
    });
    view.update(cx, |editor, cx| editor.jump_to(target, cx));
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert!(!editor.collapsed(outer_id));
        assert!(!editor.collapsed(inner_id));
        assert!(
            editor
                .visibility
                .rows
                .is_visible(source_row(editor, target))
        );
        assert_eq!(editor.buffer.cursor(), target);
        assert_eq!(editor.buffer.text(), NESTED);
    });
}

#[gpui::test]
fn vertical_arrow_keys_skip_folded_body_rows_without_opening_the_fold(cx: &mut TestAppContext) {
    let text = "Before\n\n> [!note]- Closed\n> hidden one\n> hidden two\n# After\n";
    let context = Prepared::new(text).live;
    let callout = &context.callouts()[0];
    let id = callout.callout.source.start;
    let header = callout.title.start;
    let after = text.find("# After").unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut editor = Editor::new(text, false, "", cx);
        editor.set_live_context(context, editor.buffer.revision());
        editor.set_live_preview(true, cx);
        editor.jump_to(header, cx);
        editor.focus.focus(window);
        editor
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("down");
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.line_start(editor.buffer.cursor()), after);
        assert!(editor.collapsed(id));
    });
    cx.simulate_keystrokes("up");
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.line_start(editor.buffer.cursor()), header);
        assert!(editor.collapsed(id));
        assert_eq!(editor.buffer.text(), text);
    });
}

#[gpui::test]
fn simulated_composition_keeps_utf16_candidate_geometry_in_an_indented_callout(
    cx: &mut TestAppContext,
) {
    use gpui::EntityInputHandler;
    let text =
        "Lead é😀\n\n%% begin\nhidden 👩‍💻\nend %%\n\n> [!tip]+ Édit\n> Body **😀** tail\n\nEnd\n";
    let emoji = text.rfind('😀').unwrap();
    let utf16 = text[..emoji].encode_utf16().count();
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut editor = Editor::new(text, false, "", cx);
        editor.set_live_context(Prepared::new(text).live, editor.buffer.revision());
        editor.set_live_preview(true, cx);
        editor.jump_to(text.len(), cx);
        editor.focus.focus(window);
        editor
    });
    cx.run_until_parked();
    let position = view.read_with(cx, |editor, _| {
        let body = editor
            .visible_rows
            .iter()
            .find(|row| row.projection.text == "Body 😀 tail")
            .unwrap();
        let lead = editor
            .visible_rows
            .iter()
            .find(|row| row.projection.text == "Lead é😀")
            .unwrap();
        assert!(body.origin.x > lead.origin.x);
        assert!(
            editor
                .visible_rows
                .iter()
                .all(|row| !row.projection.text.contains("hidden 👩‍💻"))
        );
        editor.point_for(emoji) + point(px(0.), body.line_height / 2.)
    });
    cx.simulate_click(position, Modifiers::default());
    view.update_in(cx, |editor, window, cx| {
        assert_eq!(editor.buffer.cursor(), emoji);
        editor.replace_and_mark_text_in_range(Some(utf16..utf16 + 2), "ni", Some(0..2), window, cx);
        assert_eq!(editor.marked_text_range(window, cx), Some(utf16..utf16 + 2));
        assert_eq!(editor.buffer.text(), text.replacen("**😀**", "**ni**", 1));
        // Supply the refreshed metadata that the workspace normally installs
        // after parsing a new revision, including during marked composition.
        editor.set_live_context(
            Prepared::new(editor.buffer.text()).live,
            editor.buffer.revision(),
        );
    });
    cx.run_until_parked();
    view.update_in(cx, |editor, window, cx| {
        let marked = utf16..utf16 + 2;
        let mut adjusted = None;
        assert_eq!(
            editor
                .text_for_range(marked.clone(), &mut adjusted, window, cx)
                .as_deref(),
            Some("ni")
        );
        assert_eq!(adjusted, Some(marked.clone()));
        let bounds = editor
            .bounds_for_range(marked, editor.bounds, window, cx)
            .unwrap();
        assert_eq!(bounds.origin, editor.point_for(emoji));
        assert!(bounds.size.height > px(0.));
        assert!(editor.bounds.contains(&bounds.center()));
        assert_eq!(
            editor.character_index_for_point(
                bounds.origin + point(px(0.), bounds.size.height / 2.),
                window,
                cx
            ),
            Some(utf16)
        );
        editor.replace_text_in_range(None, "你", window, cx);
        editor.unmark_text(window, cx);
        assert_eq!(editor.marked_text_range(window, cx), None);
        assert_eq!(editor.buffer.text(), text.replacen("**😀**", "**你**", 1));
        editor.set_live_context(
            Prepared::new(editor.buffer.text()).live,
            editor.buffer.revision(),
        );
    });
    cx.run_until_parked();
    view.update_in(cx, |editor, window, cx| {
        let end = editor.point_for(emoji + "你".len());
        assert_eq!(
            editor.character_index_for_point(end + point(px(0.), px(12.)), window, cx),
            Some(utf16 + 1)
        );
    });
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), text);
        assert_eq!(editor.buffer.cursor(), emoji);
        assert!(editor.buffer.marked.is_none());
    });
}
