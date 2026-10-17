use super::{Editor, FollowLink, LINE};
use gpui::{Modifiers, TestAppContext, point, px};

#[gpui::test]
fn document_context_renders_links_comments_and_footnotes_without_changing_source(
    cx: &mut TestAppContext,
) {
    let text = "Lead\n\nA [**é label**][ref] and note[^n].\n\n%%start\nsecret words\nend%%\n\n[ref]: Target.md\n[^n]: Footnote\n\nEnd\n";
    let context = opalglass::prepared::Prepared::new(text).live;
    let (view, cx) = cx.add_window_view(|w, cx| {
        let mut e = Editor::new(text, false, "", cx);
        e.set_live_context(context, e.buffer.revision());
        e.set_live_preview(true, cx);
        e.jump_to(text.len(), cx);
        e.focus.focus(w);
        e
    });
    cx.run_until_parked();
    view.read_with(cx, |e, _| {
        let row = e
            .visible_rows
            .iter()
            .find(|r| r.projection.text.contains("é label"))
            .unwrap();
        assert_eq!(row.projection.text, "A é label and note[1].");
        assert!(
            row.projection
                .spans
                .iter()
                .any(|s| s.style.bold && s.style.target.as_deref() == Some("Target.md"))
        );
        assert!(
            e.visible_rows
                .iter()
                .all(|r| !r.projection.text.contains("secret"))
        );
        assert_eq!(e.buffer.text(), text);
    });
    cx.simulate_keystrokes("ctrl-a ctrl-c");
    cx.update(|_, cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), text));
    cx.simulate_input("ş");
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |e, _| assert_eq!(e.buffer.text(), text));
}

#[gpui::test]
fn reference_and_footnote_control_clicks_use_context_in_both_editor_modes(cx: &mut TestAppContext) {
    use std::{cell::RefCell, rc::Rc};
    let text = "Lead\n\n[é label][ref] note[^n].\n\n[ref]: Target.md\n[^n]: Body\n\nEnd\n";
    let context = opalglass::prepared::Prepared::new(text).live;
    let (view, cx) = cx.add_window_view(|w, cx| {
        let mut e = Editor::new(text, false, "", cx);
        e.set_live_context(context, e.buffer.revision());
        e.focus.focus(w);
        e
    });
    let targets = Rc::new(RefCell::new(Vec::new()));
    let observed = targets.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &FollowLink, _| {
            observed.borrow_mut().push(event.0.clone())
        })
    });
    for live in [false, true] {
        view.update(cx, |e, cx| {
            e.set_live_preview(live, cx);
            e.jump_to(text.len(), cx);
        });
        cx.run_until_parked();
        for needle in ["label", "[^n]"] {
            let position = view.read_with(cx, |e, _| {
                e.point_for(text.find(needle).unwrap()) + point(px(1.), px(LINE / 2.))
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
        [
            "Target.md",
            "opalglass-footnote:1",
            "Target.md",
            "opalglass-footnote:1"
        ]
    );
    view.read_with(cx, |e, _| assert_eq!(e.buffer.text(), text));
}

#[gpui::test]
fn context_caret_edits_and_stale_context_preserve_unicode_source(cx: &mut TestAppContext) {
    let text = "Lead\n\n%%hidden%%[é👩‍💻][ref] suffix\n\n[ref]: Target.md\n\nEnd\n";
    let context = opalglass::prepared::Prepared::new(text).live;
    let (view, cx) = cx.add_window_view(|w, cx| {
        let mut e = Editor::new(text, false, "", cx);
        e.set_live_context(context, e.buffer.revision());
        e.set_live_preview(true, cx);
        e.jump_to(text.len(), cx);
        e.focus.focus(w);
        e
    });
    cx.run_until_parked();
    let position = view.read_with(cx, |e, _| {
        e.point_for(text.find('é').unwrap()) + point(px(0.), px(LINE / 2.))
    });
    cx.simulate_click(position, Modifiers::default());
    view.read_with(cx, |e, _| {
        assert_eq!(e.buffer.cursor(), text.find('é').unwrap())
    });
    cx.simulate_input("X");
    view.read_with(cx, |e, _| {
        assert!(e.buffer.text().contains("[Xé👩‍💻][ref]"));
        assert!(
            e.link_target_at(e.buffer.text().find('é').unwrap())
                .is_none(),
            "old metadata must not apply after an edit"
        );
    });
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |e, _| assert_eq!(e.buffer.text(), text));
}

#[gpui::test]
fn raw_footnote_urls_and_emails_follow_links_in_source_and_active_live_preview(
    cx: &mut TestAppContext,
) {
    use std::{cell::RefCell, rc::Rc};
    let text = "Lead\n\nText ^[see https://inline.example/test and inline@example.com]. Text[^n].\n\n[^n]: See https://named.example/test and named@example.com.\n\nEnd\n";
    let context = opalglass::prepared::Prepared::new(text).live;
    let (view, cx) = cx.add_window_view(|w, cx| {
        let mut e = Editor::new(text, false, "", cx);
        e.set_live_context(context, e.buffer.revision());
        e.focus.focus(w);
        e
    });
    let targets = Rc::new(RefCell::new(Vec::new()));
    let observed = targets.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &FollowLink, _| {
            observed.borrow_mut().push(event.0.clone())
        })
    });
    let links = [
        ("https://inline.example/test", "https://inline.example/test"),
        ("inline@example.com", "mailto:inline@example.com"),
        ("https://named.example/test", "https://named.example/test"),
        ("named@example.com", "mailto:named@example.com"),
    ];
    for live in [false, true] {
        for (needle, target) in links {
            let index = text.find(needle).unwrap();
            view.update(cx, |e, cx| {
                e.set_live_preview(live, cx);
                e.jump_to(index, cx);
            });
            cx.run_until_parked();
            let position =
                view.read_with(cx, |e, _| e.point_for(index) + point(px(1.), px(LINE / 2.)));
            cx.simulate_click(
                position,
                Modifiers {
                    control: true,
                    ..Default::default()
                },
            );
            assert_eq!(targets.borrow().last().map(String::as_str), Some(target));
        }
    }
    assert_eq!(targets.borrow().len(), 8);
    view.read_with(cx, |e, _| assert_eq!(e.buffer.text(), text));
}

#[gpui::test]
fn multiline_comment_urls_stay_inert_when_their_source_is_revealed(cx: &mut TestAppContext) {
    let text = "Lead\n\n%% begin\nhttps://hidden.example/path\nend %%\n\n<!-- begin\nhidden@example.com\nend -->\n\nEnd\n";
    let context = opalglass::prepared::Prepared::new(text).live;
    let (view, cx) = cx.add_window_view(|w, cx| {
        let mut e = Editor::new(text, false, "", cx);
        e.set_live_context(context, e.buffer.revision());
        e.focus.focus(w);
        e
    });
    for live in [false, true] {
        for needle in ["https://hidden.example/path", "hidden@example.com"] {
            let index = text.find(needle).unwrap();
            view.update(cx, |e, cx| {
                e.set_live_preview(live, cx);
                e.jump_to(index, cx);
            });
            cx.run_until_parked();
            view.read_with(cx, |e, _| assert_eq!(e.link_target_at(index), None));
        }
    }
}

#[gpui::test]
fn numbered_definitions_reveal_for_editing_and_preserve_copy_undo_and_link_targets(
    cx: &mut TestAppContext,
) {
    use std::{cell::RefCell, rc::Rc};
    let text = "Text[^é].\n\n[^é]: A **définition** with [link](Target.md).\n\n    Continued *body*.\n\nEnd\n";
    let context = opalglass::prepared::Prepared::new(text).live;
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut editor = Editor::new(text, false, "", cx);
        editor.set_live_context(context, editor.buffer.revision());
        editor.set_live_preview(true, cx);
        editor.jump_to(text.len(), cx);
        editor.focus.focus(window);
        editor
    });
    cx.run_until_parked();
    let observed = Rc::new(RefCell::new(Vec::new()));
    let events = observed.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &FollowLink, _| {
            events.borrow_mut().push(event.0.clone())
        })
    });
    let (number, link) = view.read_with(cx, |editor, _| {
        let definition = editor
            .visible_rows
            .iter()
            .find(|r| r.projection.text.starts_with("1. A"))
            .unwrap();
        assert_eq!(definition.projection.text, "1. A définition with link.");
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|r| r.projection.text == "Continued body.")
        );
        let number = definition.origin + point(px(1.), definition.line_height / 2.);
        let link = editor.point_for(text.find("[link]").unwrap() + 1)
            + point(px(1.), definition.line_height / 2.);
        (number, link)
    });
    for position in [number, link] {
        cx.simulate_click(
            position,
            Modifiers {
                control: true,
                ..Default::default()
            },
        );
    }
    assert_eq!(
        *observed.borrow(),
        [
            format!(
                "{}{}",
                opalglass::footnotes::BACK_PREFIX,
                text.find("[^é]").unwrap()
            ),
            "Target.md".to_owned(),
        ]
    );
    view.update(cx, |editor, cx| {
        editor.jump_to(text.find("Continued").unwrap(), cx)
    });
    cx.run_until_parked();
    view.read_with(cx, |editor, _| {
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|r| r.projection.text.starts_with("[^é]: A **"))
        );
        assert!(
            editor
                .visible_rows
                .iter()
                .any(|r| r.projection.text == "    Continued *body*.")
        );
    });
    cx.simulate_input("ş");
    cx.simulate_keystrokes("ctrl-z ctrl-a ctrl-c");
    view.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), text));
    cx.update(|_, cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), text));
}

#[gpui::test]
fn superscript_positions_follow_wrapped_glyphs_and_centered_table_alignment(
    cx: &mut TestAppContext,
) {
    use gpui::{FontWeight, TextAlign, TextRun, font, rgb};
    let (_view, cx) = cx.add_window_view(|_, cx| Editor::new("", false, "", cx));
    cx.update(|window, _| {
        let text = "[12-1]";
        let line = window
            .text_system()
            .shape_text(
                text.into(),
                px(16.),
                &[TextRun {
                    len: text.len(),
                    font: {
                        let mut font = font(crate::theme::UI_FONT);
                        font.weight = FontWeight::NORMAL;
                        font
                    },
                    color: rgb(0xb8a5fa).into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                Some(px(25.)),
                None,
            )
            .unwrap()
            .remove(0);
        let boundary = &line.wrap_boundaries[0];
        let byte = line.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index;
        let origin = crate::theme::typography::wrapped_position(
            &line,
            byte,
            px(24.),
            px(25.),
            TextAlign::Left,
        )
        .unwrap();
        assert_eq!(origin.x, px(0.));
        assert_eq!(origin.y, px(24.));
        let center = crate::theme::typography::wrapped_position(
            &line,
            byte,
            px(24.),
            px(120.),
            TextAlign::Center,
        )
        .unwrap();
        let right = crate::theme::typography::wrapped_position(
            &line,
            byte,
            px(24.),
            px(120.),
            TextAlign::Right,
        )
        .unwrap();
        assert_eq!(right.x, center.x * 2.);
        assert!(center.x > px(0.));
        assert_eq!(right.y, origin.y);
    });
}
