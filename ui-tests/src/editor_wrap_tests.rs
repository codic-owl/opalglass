use super::Editor;
use gpui::{EntityInputHandler, TestAppContext, TextRun, font, point, px, rgb, size};

#[gpui::test]
fn virtual_source_boundary_does_not_split_a_word(cx: &mut TestAppContext) {
    let text = format!("{}that's{}", "aaaa ".repeat(51), " tail".repeat(13));
    assert_eq!(text.find("that's"), Some(255));
    assert_eq!(text.len(), 326);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let editor = Editor::new(&text, false, "", cx);
        editor.focus.focus(window);
        editor
    });
    cx.simulate_resize(size(px(720.), px(1200.)));
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert_eq!(
            editor.point_for(255).y,
            editor.point_for(256).y,
            "an internal source chunk must not split that's"
        );
        assert_eq!(editor.buffer.text(), text);
    });
}

#[gpui::test]
fn source_flow_matches_unsplit_native_layout_at_every_character(cx: &mut TestAppContext) {
    let text = format!(
        "{}that's {}",
        "aaaa ".repeat(51),
        "é 👩‍💻 e\u{301} words ".repeat(25)
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        let editor = Editor::new(&text, false, "", cx);
        editor.focus.focus(window);
        editor
    });
    for width in [420., 720., 1600.] {
        cx.simulate_resize(size(px(width), px(6000.)));
        for font_size in [10., 14., 28.] {
            view.update(cx, |editor, cx| editor.set_preferences(font_size, true, cx));
            cx.run_until_parked();
            view.update_in(cx, |editor, window, _| {
                let reference = window
                    .text_system()
                    .shape_text(
                        text.clone().into(),
                        px(font_size),
                        &[TextRun {
                            len: text.len(),
                            font: font(crate::theme::MONO_FONT),
                            color: rgb(0xd6d9e3).into(),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        }],
                        Some(editor.source_width),
                        None,
                    )
                    .unwrap()
                    .remove(0);
                let origin = editor.point_for(0);
                let height = px(super::LINE * font_size / 14.);
                for (byte, _) in text.char_indices() {
                    let actual = editor.point_for(byte) - origin;
                    let expected = reference.position_for_index(byte, height).unwrap();
                    // Whole-line shaping accumulates f32 glyph advances over
                    // the entire line before subtracting its wrap origin;
                    // bounded windows restart that sum. Keep y within 0.001px
                    // and x within 1/32px, far below a displayed glyph/pixel.
                    assert!(
                        (f32::from(actual.x - expected.x)).abs() < 1. / 32.
                            && (f32::from(actual.y - expected.y)).abs() < 0.001,
                        "width={width} font={font_size} byte={byte}: {actual:?} != {expected:?}"
                    );
                }
                assert_eq!(editor.buffer.text(), text);
            });
        }
    }
}

#[gpui::test]
fn source_flow_preserves_boundary_input_clipboard_and_physical_newlines(cx: &mut TestAppContext) {
    let text = format!(
        "{}that's tail\n{}",
        "aaaa ".repeat(51),
        "second physical line ".repeat(20)
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        let editor = Editor::new(&text, false, "", cx);
        editor.focus.focus(window);
        editor
    });
    cx.simulate_resize(size(px(1600.), px(1000.)));
    cx.run_until_parked();
    let position = view.read_with(cx, |editor, _| {
        editor.point_for(256) + point(px(0.), px(super::LINE / 2.))
    });
    cx.simulate_click(position, Default::default());
    view.read_with(cx, |editor, _| assert_eq!(editor.buffer.cursor(), 256));
    view.update(cx, |editor, cx| {
        editor.buffer.select(255, 261);
        editor.reveal_cursor(cx);
    });
    cx.simulate_keystrokes("ctrl-c");
    cx.update(|_, cx| {
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("that's")
        )
    });
    view.update_in(cx, |editor, window, cx| {
        let range = editor.buffer.to_utf16(255..261);
        editor.replace_and_mark_text_in_range(Some(range), "ni", Some(0..2), window, cx);
        editor.replace_text_in_range(None, "你", window, cx);
    });
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), text));
    view.update(cx, |editor, cx| {
        editor.jump_to(text.find('\n').unwrap(), cx);
    });
    cx.simulate_keystrokes("down");
    view.read_with(cx, |editor, _| {
        assert!(editor.buffer.cursor() > text.find('\n').unwrap());
        assert_eq!(editor.buffer.text(), text);
    });
}

#[gpui::test]
fn long_line_jump_keeps_exact_text_and_bounds_checkpoint_work(cx: &mut TestAppContext) {
    let text = "words é 👩‍💻 e\u{301} ".repeat(10_000);
    let started = std::time::Instant::now();
    let (view, cx) = cx.add_window_view(|window, cx| {
        let editor = Editor::new(&text, false, "", cx);
        editor.focus.focus(window);
        editor
    });
    cx.simulate_keystrokes("ctrl-end");
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.cursor(), text.len());
        assert_eq!(editor.buffer.text(), text);
        let caret = editor.point_for(text.len());
        assert!(caret.y >= editor.bounds.top() && caret.y < editor.bounds.bottom());
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|row| row.range.end == text.len() && !row.range.is_empty()),
            "target text must actually be painted"
        );
        let (rows, checkpoints, remaining) = editor.source_flow.cache_counts();
        assert!(rows <= 256);
        assert!(checkpoints <= 512);
        assert!(remaining <= 128);
    });
    assert!(
        started.elapsed() < std::time::Duration::from_secs(1),
        "long-line jump took {:?}",
        started.elapsed()
    );
}

#[gpui::test]
fn fresh_sixteen_megabyte_end_jump_paints_target_without_prefix_frames(cx: &mut TestAppContext) {
    let text = "words that wrap ".repeat(1_048_576);
    assert_eq!(text.len(), 16 * 1024 * 1024);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let editor = Editor::new("", false, "", cx);
        editor.focus.focus(window);
        editor
    });
    let before = view.read_with(cx, |editor, _| editor.render_count);
    let started = std::time::Instant::now();
    view.update(cx, |editor, cx| {
        editor.set_text(&text, cx);
        editor.jump_to(text.len(), cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), text);
        assert_eq!(editor.buffer.cursor(), text.len());
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|row| row.range.end == text.len() && !row.range.is_empty()),
            "huge-line target text must be painted: frames={} prepared={} cache={:?} source_complete={} elapsed={:?} visible={:?} scroll={:?}", editor.render_count-before, editor.source_flow.prepared_count(), editor.source_flow.cache_counts(), editor.source_complete, started.elapsed(), editor.visible_rows.iter().map(|row|row.range.clone()).collect::<Vec<_>>(),editor.source_list.logical_scroll_top()
        );
        let caret = editor.point_for(text.len());
        assert!(caret.y >= editor.bounds.top() && caret.y < editor.bounds.bottom());
        assert!(
            editor.render_count - before <= 8,
            "a jump must not walk its prefix one display frame at a time"
        );
        let (rows, checkpoints, _) = editor.source_flow.cache_counts();
        assert!(rows <= 256 && checkpoints <= 512);
        assert_eq!(editor.first_paint_indexed_rows, Some(1));
    });
    assert!(
        started.elapsed() < std::time::Duration::from_secs(1),
        "huge-line jump took {:?}",
        started.elapsed()
    );
}

#[gpui::test]
fn background_checkpoint_cuts_match_native_wraps_after_zoom(cx: &mut TestAppContext) {
    let text = "some é words 👩‍💻 e\u{301} that's more text ".repeat(1500);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let editor = Editor::new(&text, false, "", cx);
        editor.focus.focus(window);
        editor
    });
    cx.simulate_resize(size(px(720.), px(1000.)));
    for font_size in [14., 28.] {
        view.update(cx, |editor, cx| {
            editor.set_preferences(font_size, true, cx);
            editor.jump_to(text.len(), cx);
        });
        cx.run_until_parked();
        view.update_in(cx, |editor, window, _| {
            assert!(editor.source_flow.prepared_count() > 0);
            let reference = window
                .text_system()
                .shape_text(
                    text.clone().into(),
                    px(font_size),
                    &[TextRun {
                        len: text.len(),
                        font: font(crate::theme::MONO_FONT),
                        color: rgb(0xd6d9e3).into(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    Some(editor.source_width),
                    None,
                )
                .unwrap()
                .remove(0);
            let mut cuts: Vec<_> = reference
                .wrap_boundaries
                .iter()
                .map(|boundary| {
                    reference.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index
                })
                .collect();
            cuts.extend([0, text.len()]);
            cuts.sort_unstable();
            for row in &editor.visible_rows {
                assert!(
                    cuts.binary_search(&row.range.start).is_ok(),
                    "non-native start {:?}",
                    row.range
                );
                assert!(
                    cuts.binary_search(&row.range.end).is_ok(),
                    "non-native end {:?}",
                    row.range
                );
                assert_eq!(row.projection.text.as_str(), &text[row.range.clone()]);
            }
            assert!(
                editor
                    .visible_rows
                    .iter()
                    .any(|row| row.range.end == text.len())
            );
        });
    }
}

#[gpui::test]
fn replacing_buffer_cancels_prepared_ranges_even_when_revision_resets(cx: &mut TestAppContext) {
    let old = "old words 👩‍💻 ".repeat(20_000);
    let replacement = format!("{}that's new", "aaaa ".repeat(51));
    let (view, cx) = cx.add_window_view(|window, cx| {
        let editor = Editor::new("", false, "", cx);
        editor.focus.focus(window);
        editor
    });
    view.update(cx, |editor, cx| {
        editor.set_text(&old, cx);
        editor.prepare_source();
        editor.source_flow.begin_frame(
            editor.buffer.revision(),
            editor.source_width,
            editor.font_size,
        );
        editor.start_source_flow_index(cx);
        editor.set_text(&replacement, cx);
        editor.jump_to(replacement.len(), cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), replacement);
        assert_eq!(editor.source_flow.prepared_count(), 0);
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|row| row.range.end == replacement.len())
        );
        assert!(
            editor
                .cached_rows
                .iter()
                .all(|row| row.range.end <= replacement.len())
        );
    });
}
