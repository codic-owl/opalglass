use super::*;

#[gpui::test]
fn repeated_nested_and_same_note_embeds_keep_distinct_widgets_and_stop_cycles(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(
        folder.path().join("Host.md"),
        "# Host\n\n![[B]]\n\n![[B]]\n\n![[#Local]]\n\n# Local\n\n```txt\nLOCAL\n```\n\n# End\n",
    )
    .unwrap();
    std::fs::write(
        folder.path().join("B.md"),
        "```txt\nB BODY\n```\n\n![[C]]\n",
    )
    .unwrap();
    std::fs::write(
        folder.path().join("C.md"),
        "```txt\nC BODY\n```\n\n![[B]]\n",
    )
    .unwrap();
    let (view, cx) = reading(cx, folder.path());
    let roots = instances(&view, cx, "B")
        .into_iter()
        .filter(|instance| instance.parent == 0)
        .collect::<Vec<_>>();
    assert_eq!(roots.len(), 2);
    assert_ne!(roots[0].id, roots[1].id);
    for root in &roots {
        show_instance(&view, cx, root.id);
        copy_code(&view, cx, root.id, "B BODY\n");
    }
    let child = instances(&view, cx, "C")
        .into_iter()
        .find(|instance| instance.parent == roots[0].id)
        .unwrap();
    show_instance(&view, cx, child.id);
    copy_code(&view, cx, child.id, "C BODY\n");
    let cycle = instances(&view, cx, "B")
        .into_iter()
        .find(|instance| instance.parent == child.id)
        .unwrap();
    assert_eq!(cycle.error.as_deref(), Some("Circular note embed"));
    show_instance(&view, cx, cycle.id);
    assert!(
        cx.debug_bounds(Box::leak(
            format!("embed-header-{}", cycle.id).into_boxed_str()
        ))
        .is_some()
    );
    view.read_with(cx, |this, cx| {
        assert!(this.tabs[this.active].composition.projection.len() < 100);
        // The visible cycle error still owns its containing documents' leases.
        for id in [roots[0].id, child.id] {
            let instance = &this.tabs[this.active].composition.instances[&id];
            let input = instance.input.as_ref().unwrap();
            assert!(
                this.embedded_documents
                    .read(cx)
                    .ready(&input.path, input.token)
                    .is_some()
            );
        }
    });
    let local = instances(&view, cx, "#Local")[0].id;
    show_instance(&view, cx, local);
    copy_code(&view, cx, local, "LOCAL\n");
    view.read_with(cx, |this, _| {
        assert!(
            this.tabs[this.active].composition.instances[&local]
                .error
                .is_none()
        );
        assert_eq!(
            this.tabs[this.active].path,
            std::path::PathBuf::from("Host.md")
        );
    });
}

#[gpui::test]
fn embedded_section_footnotes_render_external_definitions_and_click_within_their_instance(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    let host = "# Host\n\nHost[^h].\n\n![[Source#Part]]\n\n![[Source#Part]]\n\n[^h]: Host footer\n";
    let source = "# Part\n\n| Reference |\n| --- |\n| [^n] |\n\n# Other\n\n[^n]:\n    ```txt\n    EMBEDDED FOOTER é\n    ```\n";
    std::fs::write(folder.path().join("Host.md"), host).unwrap();
    std::fs::write(folder.path().join("Source.md"), source).unwrap();
    let (view, cx) = reading(cx, folder.path());
    let repeated = instances(&view, cx, "Source#Part");
    assert_eq!(repeated.len(), 2);
    let id = repeated[1].id;
    show_instance(&view, cx, id);
    let source_doc = document(&view, cx, id);
    let reference = source_doc
        .prepared
        .rows
        .iter()
        .position(|row| row.table.as_ref().is_some_and(|table| !table.row.header))
        .unwrap();
    show_row(&view, cx, id, reference);
    let caret = view.read_with(cx, |this, cx| {
        this.tabs[this.active].editor.read(cx).buffer.cursor()
    });
    let cell = cx
        .debug_bounds(Box::leak(
            format!("embed-{id}-reading-table-{reference}-0").into_boxed_str(),
        ))
        .unwrap();
    cx.simulate_click(cell.origin + point(px(18.), px(14.)), Modifiers::default());
    settle(cx);
    let definition = source_doc
        .prepared
        .footnotes
        .definitions
        .iter()
        .find(|definition| definition.number == 1)
        .unwrap();
    let last = definition.sections.last().unwrap().end - 1;
    let back = format!("embed-{id}-footnote-return-{last}-0");
    assert!(
        cx.debug_bounds(Box::leak(back.clone().into_boxed_str()))
            .is_some(),
        "reference must reveal this instance's footer"
    );
    copy_code(&view, cx, id, "EMBEDDED FOOTER é\n");
    click_selector(cx, back);
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        let logical = tab.composition.instances[&id]
            .projection
            .list_index(reference);
        let target = tab
            .composition
            .projection
            .find_document_row(id, logical)
            .unwrap();
        assert!(tab.preview_scroll.bounds_for_item(target).is_some());
        assert_eq!(
            tab.editor.read(cx).buffer.cursor(),
            caret,
            "embedded footnotes must not move the host editor caret"
        );
        assert_eq!(tab.path, std::path::PathBuf::from("Host.md"));
        assert_eq!(tab.editor.read(cx).buffer.text(), host);
    });
}
