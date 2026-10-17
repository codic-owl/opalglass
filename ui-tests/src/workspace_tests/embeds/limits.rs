use super::*;

#[gpui::test]
fn depth_limit_keeps_a_visible_placeholder_that_opens_the_original_note(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let host = "# Host\n\n![[Level1]]\n";
    std::fs::write(folder.path().join("Host.md"), host).unwrap();
    for depth in 1..=9 {
        let next = if depth < 9 {
            format!("\n![[Level{}]]\n", depth + 1)
        } else {
            String::new()
        };
        std::fs::write(
            folder.path().join(format!("Level{depth}.md")),
            format!("# Level {depth}\n\n```txt\nLEVEL {depth}\n```\n{next}"),
        )
        .unwrap();
    }
    let (view, cx) = reading(cx, folder.path());
    let mut ancestors = Vec::new();
    for depth in 1..=8 {
        let id = instances(&view, cx, &format!("Level{depth}"))[0].id;
        show_instance(&view, cx, id);
        copy_code(&view, cx, id, &format!("LEVEL {depth}\n"));
        ancestors.push(id);
    }
    let stopped = instances(&view, cx, "Level9")[0].clone();
    assert_eq!(
        stopped.error.as_deref(),
        Some("Maximum embed depth reached")
    );
    show_instance(&view, cx, stopped.id);
    assert!(
        cx.debug_bounds(Box::leak(
            format!("embed-header-{}", stopped.id).into_boxed_str()
        ))
        .is_some()
    );
    for id in ancestors {
        let _ = document(&view, cx, id);
    }
    click_selector(cx, format!("open-embed-{}", stopped.id));
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        assert_eq!(tab.path, std::path::PathBuf::from("Level9.md"));
        assert!(tab.editor.read(cx).buffer.text().contains("LEVEL 9"));
        let host_tab = this
            .tabs
            .iter()
            .find(|tab| tab.path == std::path::PathBuf::from("Host.md"))
            .unwrap();
        assert_eq!(host_tab.editor.read(cx).buffer.text(), host);
    });
}

#[gpui::test]
fn folding_an_embedded_callout_above_the_viewport_preserves_the_visible_tail(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    let host = "# Host\n\n![[Source]]\n";
    let source = format!(
        "# Introduction\n\n> [!note]+ Long callout\n{}\n# Tail\n\n```txt\nTAIL é 👩‍💻\n```\n\n{}",
        "> Body paragraph inside the callout.\n>\n".repeat(300),
        "After tail paragraph.\n\n".repeat(100)
    );
    std::fs::write(folder.path().join("Host.md"), host).unwrap();
    std::fs::write(folder.path().join("Source.md"), source).unwrap();
    let (view, cx) = reading(cx, folder.path());
    let id = instances(&view, cx, "Source")[0].id;
    show_instance(&view, cx, id);
    let doc = document(&view, cx, id);
    let callout = doc.prepared.callouts[0].clone();
    show_row(&view, cx, id, callout.rows.start);
    let selector = format!("embed-{id}-fold-callout-{}", callout.rows.start);
    click_selector(cx, selector.clone());
    view.read_with(cx, |this, _| {
        assert!(
            this.tabs[this.active].composition.instances[&id]
                .projection
                .collapsed(&callout)
        )
    });
    click_selector(cx, selector);
    copy_code(&view, cx, id, "TAIL é 👩‍💻\n");
    let before = view.read_with(cx, |this, _| {
        let tab = &this.tabs[this.active];
        let top = tab.preview_scroll.logical_scroll_top();
        let Some(Row::Document { instance, index }) = tab.composition.projection.row(top.item_ix)
        else {
            panic!("tail must be a document row")
        };
        assert_eq!(instance, id);
        (
            tab.composition.instances[&id].projection.row(index),
            f32::from(top.offset_in_item),
            top.item_ix,
        )
    });
    // Dispatch the same action as the verified fold button while the heading
    // is offscreen, so the transition itself must preserve the visible row.
    view.update(cx, |this, cx| {
        let context = this.reading_context_for(id, cx).unwrap();
        this.toggle_reading_callout(context, callout.source.start, false, cx);
    });
    settle(cx);
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        let top = tab.preview_scroll.logical_scroll_top();
        let Some(Row::Document { instance, index }) = tab.composition.projection.row(top.item_ix)
        else {
            panic!("tail must remain a document row")
        };
        assert_eq!(instance, id);
        assert_eq!(
            tab.composition.instances[&id].projection.row(index),
            before.0
        );
        assert!(top.item_ix + 200 < before.2);
        assert!((f32::from(top.offset_in_item) - before.1).abs() < 1.);
        assert!(
            tab.composition.instances[&id]
                .projection
                .collapsed(&callout)
        );
        assert_eq!(tab.editor.read(cx).buffer.text(), host);
    });
    click_selector(cx, format!("embed-{id}-copy-code-{}", before.0));
    cx.update(|_, cx| {
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().unwrap(),
            "TAIL é 👩‍💻\n"
        )
    });
}
