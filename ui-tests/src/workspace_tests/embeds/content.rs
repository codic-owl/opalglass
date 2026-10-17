use super::*;

#[gpui::test]
fn embed_full_notes_sections_blocks_and_inline_fragments_reach_real_widgets(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    let host = "# Host\n\nBefore ![[Source#Part]] After ![40](Photo.svg) ending.\n\n![[Source#^chosen]]\n\n![[Source]]\n";
    let source = "# Intro\n\n```txt\nINTRO\n```\n\n# Part\n\n```txt\nPART é 👩‍💻\n```\n\n```txt\nBLOCK\n```\n^chosen\n\n# End\n\n```txt\nEND\n```\n";
    std::fs::write(folder.path().join("Host.md"), host).unwrap();
    std::fs::write(folder.path().join("Source.md"), source).unwrap();
    std::fs::write(folder.path().join("Photo.svg"), PHOTO).unwrap();
    let (view, cx) = reading(cx, folder.path());
    let part = instances(&view, cx, "Source#Part")[0].id;
    show_instance(&view, cx, part);
    copy_code(&view, cx, part, "PART é 👩‍💻\n");
    let block = instances(&view, cx, "Source#^chosen")[0].id;
    show_instance(&view, cx, block);
    copy_code(&view, cx, block, "BLOCK\n");
    let full = instances(&view, cx, "Source")[0].id;
    show_instance(&view, cx, full);
    copy_code(&view, cx, full, "INTRO\n");
    copy_code(&view, cx, full, "END\n");
    // Prefix/suffix content survives on both sides of an inline transclusion;
    // its neighboring image must still use the native image widget.
    scroll_to(&view, cx, 0);
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        let fragments = (0..tab.composition.projection.len())
            .filter_map(|index| {
                if let Some(Row::Fragment {
                    instance: 0,
                    index,
                    range,
                    ..
                }) = tab.composition.projection.row(index)
                {
                    Some(tab.prepared.rows[index].text[range].to_owned())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert!(fragments.iter().any(|text| text == "Before "));
        assert!(
            fragments
                .iter()
                .any(|text| text.starts_with(" After ") && text.ends_with(" ending."))
        );
        assert_eq!(tab.editor.read(cx).buffer.text(), host);
    });
    let image = cx
        .debug_bounds("attachment-image-1-0")
        .or_else(|| cx.debug_bounds("attachment-image-1-1"));
    assert!(
        image.is_some(),
        "the inline suffix must retain its local image widget"
    );
}

#[gpui::test]
fn embedded_relative_images_and_reference_links_use_the_source_directory(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("Nested")).unwrap();
    std::fs::write(folder.path().join("Host.md"), "![[Nested/Source#Part]]\n").unwrap();
    std::fs::write(folder.path().join("Nested/Source.md"), "# Part\n\n![[Photo.svg|100]]\n\n| Target |\n| --- |\n| [Go][dest] |\n\n# Other\n\n[dest]: Target.md\n").unwrap();
    std::fs::write(folder.path().join("Nested/Target.md"), "# Nested target\n").unwrap();
    std::fs::write(folder.path().join("Target.md"), "# Wrong root target\n").unwrap();
    std::fs::write(folder.path().join("Nested/Photo.svg"), PHOTO).unwrap();
    std::fs::write(
        folder.path().join("Photo.svg"),
        PHOTO.replace("height=\"100\"", "height=\"200\""),
    )
    .unwrap();
    let (view, cx) = reading(cx, folder.path());
    let id = instances(&view, cx, "Nested/Source#Part")[0].id;
    show_instance(&view, cx, id);
    let doc = document(&view, cx, id);
    let image_row = doc
        .prepared
        .rows
        .iter()
        .position(|row| row.spans.iter().any(|span| span.style.image))
        .unwrap();
    show_row(&view, cx, id, image_row);
    let image = cx
        .debug_bounds(Box::leak(
            format!("embed-{id}-attachment-image-{image_row}-0").into_boxed_str(),
        ))
        .unwrap();
    assert!((f32::from(image.size.width) - 100.).abs() < 1.);
    assert!(
        (f32::from(image.size.height) - 50.).abs() < 1.,
        "image must resolve inside Nested, not the square root image"
    );
    let table = doc
        .prepared
        .rows
        .iter()
        .position(|row| row.table.as_ref().is_some_and(|table| !table.row.header))
        .unwrap();
    show_row(&view, cx, id, table);
    let cell = cx
        .debug_bounds(Box::leak(
            format!("embed-{id}-reading-table-{table}-0").into_boxed_str(),
        ))
        .unwrap();
    cx.simulate_click(cell.origin + point(px(28.), px(14.)), Modifiers::default());
    settle(cx);
    view.read_with(cx, |this, _| {
        assert_eq!(
            this.tabs[this.active].path,
            std::path::PathBuf::from("Nested/Target.md")
        )
    });
}
