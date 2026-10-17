use super::*;

#[gpui::test]
fn attachment_paste_at_start_preserves_frontmatter_and_deleted_note_can_be_restored(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    let source = "---\ntags: [work]\n---\nBody\n";
    let path = folder.path().join("A.md");
    std::fs::write(&path, source).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    view.update_in(cx, |this, window, cx| {
        this.import_attachments(
            this.tabs[this.active].editor.clone(),
            vec![opalglass::attachments::Input::Bytes {
                name: "Test.pdf".into(),
                bytes: b"%PDF-1.7".to_vec(),
            }],
            window,
            cx,
        );
    });
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        let text = this.tabs[this.active].editor.read(cx).buffer.text();
        assert!(text.starts_with("---\ntags: [work]\n---\n[Test.pdf]"));
        assert!(text.ends_with("Body\n"));
    });
    std::fs::remove_file(&path).unwrap();
    view.update_in(cx, |this, window, cx| this.open_comparison(window, cx));
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert!(this.comparison.as_ref().unwrap().deleted)
    });
    click(cx, "comparison-save-ours");
    cx.run_until_parked();
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .starts_with("---\ntags: [work]\n---\n[Test.pdf]")
    );
}

#[gpui::test]
fn unlinked_mentions_link_saved_text_and_refuse_unsaved_or_stale_sources(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(
        folder.path().join("A.md"),
        "A mentions Target twice: Target. [[Target]] `Target`\n",
    )
    .unwrap();
    std::fs::write(folder.path().join("Target.md"), "Target note").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    view.update_in(cx, |this, window, cx| {
        this.open_note("Target.md".into(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |this, window, cx| this.open_mentions(window, cx));
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert_eq!(this.mentions.as_ref().unwrap().items.len(), 2)
    });
    click(cx, "link-mention-0");
    cx.run_until_parked();
    assert!(
        std::fs::read_to_string(folder.path().join("A.md"))
            .unwrap()
            .contains("[[/Target|Target]]")
    );
    view.read_with(cx, |this, _| {
        assert_eq!(this.mentions.as_ref().unwrap().items.len(), 1)
    });
    std::fs::write(folder.path().join("A.md"), "External Target change").unwrap();
    click(cx, "link-mention-0");
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert!(
            this.mentions
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .contains("changed")
        )
    });
    assert_eq!(
        std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
        "External Target change"
    );
    view.update(cx, |this, cx| {
        let tab = this
            .tabs
            .iter()
            .find(|t| t.path == PathBuf::from("A.md"))
            .unwrap();
        tab.editor
            .update(cx, |e, cx| e.set_text("Unsaved Target change", cx));
    });
    view.update_in(cx, |this, window, cx| this.open_mentions(window, cx));
    cx.run_until_parked();
    click(cx, "link-mention-0");
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert!(
            this.mentions
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .contains("unsaved")
        )
    });
}

#[gpui::test]
fn workspace_replace_requires_current_preview_checks_all_files_and_respects_selection(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    for (path, text) in [
        ("A.md", "apple and APPLE\n"),
        ("B.md", "apple\n"),
        ("C.md", "untouched\n"),
    ] {
        std::fs::write(folder.path().join(path), text).unwrap();
    }
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-shift-h");
    view.update(cx, |this, cx| {
        let panel = this.replace_panel.as_mut().unwrap();
        panel.find.update(cx, |e, cx| e.set_text("apple", cx));
        panel.replacement.update(cx, |e, cx| e.set_text("pear", cx));
    });
    click(cx, "replace-preview");
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
        "apple and APPLE\n"
    );
    view.read_with(cx, |this, _| {
        let panel = this.replace_panel.as_ref().unwrap();
        assert_eq!(panel.plans.len(), 2);
        assert_eq!(panel.plans[0].edits.len(), 2);
        assert!(
            panel.plans[0]
                .preview
                .iter()
                .any(|r| r.text == "pear and pear")
        );
    });
    view.update(cx, |this, cx| {
        this.replace_panel
            .as_mut()
            .unwrap()
            .replacement
            .update(cx, |e, cx| e.set_text("banana", cx))
    });
    click(cx, "replace-apply");
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
        "apple and APPLE\n"
    );
    click(cx, "replace-preview");
    cx.run_until_parked();
    std::fs::write(folder.path().join("B.md"), "External apple change\n").unwrap();
    click(cx, "replace-apply");
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
        "apple and APPLE\n"
    );
    view.read_with(cx, |this, _| {
        assert!(
            this.replace_panel
                .as_ref()
                .unwrap()
                .message
                .as_ref()
                .unwrap()
                .contains("changed")
        )
    });
    click(cx, "replace-preview");
    cx.run_until_parked();
    click(cx, "replace-select-1");
    click(cx, "replace-apply");
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
        "banana and banana\n"
    );
    assert_eq!(
        std::fs::read_to_string(folder.path().join("B.md")).unwrap(),
        "External apple change\n"
    );
    assert_eq!(
        std::fs::read_to_string(folder.path().join("C.md")).unwrap(),
        "untouched\n"
    );
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[0].editor.read(cx).buffer.text(),
            "banana and banana\n"
        )
    });
}

#[gpui::test]
fn screenshot_paste_and_attachment_import_insert_resolvable_links_and_undo(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("Nested")).unwrap();
    std::fs::write(folder.path().join("Nested/A.md"), "Note\n").unwrap();
    let external = tempfile::tempdir().unwrap();
    let pdf = external.path().join("Reference (one).pdf");
    std::fs::write(&pdf, "%PDF-1.7\nfixture").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    view.update_in(cx, |this, window, cx| {
        this.set_mode(super::ViewMode::Source, window, cx);
        let editor = &this.tabs[this.active].editor;
        editor.update(cx, |e, cx| e.jump_to(e.buffer.text().len(), cx));
        cx.write_to_clipboard(gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(
            gpui::ImageFormat::Png,
            vec![137, 80, 78, 71, 13, 10, 26, 10],
        )));
    });
    cx.simulate_keystrokes("ctrl-v");
    cx.run_until_parked();
    assert_eq!(
        std::fs::read(folder.path().join("Attachments/Screenshot.png")).unwrap(),
        vec![137, 80, 78, 71, 13, 10, 26, 10]
    );
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        let text = tab.editor.read(cx).buffer.text();
        assert!(text.contains("![Screenshot.png](../Attachments/Screenshot.png)"));
        assert!(tab.dirty(cx));
    });
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.text(),
            "Note\n"
        )
    });
    view.update_in(cx, |this, window, cx| {
        this.import_attachments(
            this.tabs[this.active].editor.clone(),
            vec![opalglass::attachments::Input::File(pdf.clone())],
            window,
            cx,
        );
    });
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        let text = this.tabs[this.active].editor.read(cx).buffer.text();
        assert!(text.contains("[Reference -one-.pdf](../Attachments/Reference%20-one-.pdf)"));
        assert!(!text.contains("![Reference"));
    });
    assert_eq!(std::fs::read_to_string(&pdf).unwrap(), "%PDF-1.7\nfixture");
    cx.simulate_keystrokes("ctrl-s");
    assert!(
        std::fs::read_to_string(folder.path().join("Nested/A.md"))
            .unwrap()
            .contains("Attachments/Reference")
    );
}

#[gpui::test]
fn properties_fields_yaml_switch_undo_validation_and_stale_edits(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let source = "---\n# preserve this comment\ntitle: Original\ntags: [work]\n---\n\n# Body\nUntouched text.\n";
    std::fs::write(folder.path().join("A.md"), source).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    click(cx, "properties");
    view.update(cx, |this, cx| {
        let panel = this.properties_panel.as_mut().unwrap();
        panel.rows[0]
            .value
            .update(cx, |e, cx| e.set_text("A much longer title", cx));
        this.switch_properties_mode(cx);
    });
    view.read_with(cx, |this, cx| {
        let panel = this.properties_panel.as_ref().unwrap();
        assert!(panel.raw);
        assert!(
            panel
                .yaml
                .read(cx)
                .buffer
                .text()
                .contains("# preserve this comment")
        );
        assert!(
            panel
                .yaml
                .read(cx)
                .buffer
                .text()
                .contains("A much longer title")
        );
    });
    view.update(cx, |this, cx| this.switch_properties_mode(cx));
    click(cx, "apply-properties");
    view.read_with(cx, |this, cx| {
        assert!(this.properties_panel.is_none());
        let text = this.tabs[this.active].editor.read(cx).buffer.text();
        assert!(text.ends_with("---\n\n# Body\nUntouched text.\n"));
        assert!(text.contains("title: A much longer title"));
        assert!(text.contains("# preserve this comment"));
    });
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[this.active].editor.read(cx).buffer.text(), source)
    });
    click(cx, "properties");
    view.update(cx, |this, cx| {
        this.properties_panel.as_mut().unwrap().rows[0]
            .value
            .update(cx, |e, cx| e.set_text("[invalid", cx))
    });
    click(cx, "apply-properties");
    view.read_with(cx, |this, cx| {
        assert!(this.properties_panel.as_ref().unwrap().error.is_some());
        assert_eq!(this.tabs[this.active].editor.read(cx).buffer.text(), source);
    });
    cx.simulate_keystrokes("escape");
    click(cx, "properties");
    view.update(cx, |this, cx| {
        this.tabs[this.active]
            .editor
            .update(cx, |e, cx| e.set_text("New external contents", cx))
    });
    click(cx, "apply-properties");
    view.read_with(cx, |this, cx| {
        assert!(
            this.properties_panel
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .contains("changed")
        );
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.text(),
            "New external contents"
        );
    });
}

#[gpui::test]
fn command_palette_bookmarks_daily_notes_and_templates(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("Templates")).unwrap();
    std::fs::write(folder.path().join("A.md"), "Alpha").unwrap();
    std::fs::write(folder.path().join("B.md"), "Beta").unwrap();
    std::fs::write(
        folder.path().join("Templates/Meeting.md"),
        "# {{title}}\nDate: {{date}} at {{time}}\n- [ ] Follow up\n",
    )
    .unwrap();
    std::fs::write(
        folder.path().join("Templates/Daily.md"),
        "# {{date}}\nDaily template\n",
    )
    .unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    view.update(cx, |this, _| {
        this.state_directory = Some(state.path().into())
    });
    cx.simulate_keystrokes("ctrl-a ctrl-shift-p");
    view.read_with(cx, |this, _| {
        assert!(this.picker == Some(super::PickerMode::Commands))
    });
    cx.simulate_input("format bold");
    view.read_with(cx, |this, cx| {
        assert_eq!(this.input.read(cx).buffer.text(), "format bold");
        assert_eq!(this.picker_items.len(), 1);
        assert_eq!(this.picker_items[0].title, "Format: bold");
    });
    cx.simulate_keystrokes("enter");
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.text(),
            "**Alpha**"
        )
    });
    cx.simulate_keystrokes("ctrl-z ctrl-shift-d");
    view.read_with(cx, |this, _| {
        assert_eq!(this.navigation.bookmarks, vec![PathBuf::from("A.md")]);
        assert_eq!(
            opalglass::navigation::State::load(state.path(), &this.vault.as_ref().unwrap().root)
                .bookmarks,
            this.navigation.bookmarks
        );
    });
    view.update_in(cx, |this, w, cx| this.open_note("B.md".into(), w, cx));
    cx.run_until_parked();
    click(cx, "bookmarks");
    click(cx, "picker-option-0");
    view.read_with(cx, |this, _| {
        assert_eq!(this.tabs[this.active].path, PathBuf::from("A.md"))
    });
    cx.simulate_keystrokes("ctrl-end ctrl-alt-t");
    cx.simulate_input("meeting");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        let text = this.tabs[this.active].editor.read(cx).buffer.text();
        assert!(text.contains("# A\nDate: "));
        assert!(!text.contains("{{"));
    });
    cx.simulate_keystrokes("ctrl-z");
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.text(),
            "Alpha"
        )
    });
    cx.simulate_keystrokes("ctrl-alt-d");
    cx.run_until_parked();
    let path = view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        assert!(tab.path.starts_with("Daily"));
        assert!(tab.editor.read(cx).buffer.text().contains("Daily template"));
        tab.path.clone()
    });
    cx.simulate_keystrokes("ctrl-end");
    cx.simulate_input("Keep my edits");
    cx.simulate_keystrokes("ctrl-alt-d");
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[this.active].path, path);
        assert!(
            this.tabs[this.active]
                .editor
                .read(cx)
                .buffer
                .text()
                .ends_with("Keep my edits")
        );
        assert_eq!(this.tabs.iter().filter(|t| t.path == path).count(), 1);
    });
}

#[gpui::test]
fn explorer_renames_and_moves_attachments_with_markdown_links(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("Archive")).unwrap();
    std::fs::write(folder.path().join("A.md"), "[reference](Reference.pdf)\n").unwrap();
    std::fs::write(folder.path().join("Reference.pdf"), "%PDF-1.7\nfixture").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    assert!(cx.debug_bounds("entry:Reference.pdf").is_some());
    view.update_in(cx, |this, window, cx| {
        this.navigation.selected = Some("Reference.pdf".into());
        this.show_inline(super::Modal::Rename, window, cx);
    });
    view.read_with(cx, |this, cx| {
        assert_eq!(this.input.read(cx).buffer.text(), "Reference.pdf")
    });
    cx.simulate_input("Renamed.pdf");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(folder.path().join("Renamed.pdf").is_file());
    assert!(!folder.path().join("Renamed.pdf.md").exists());
    assert_eq!(
        std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
        "[reference](Renamed.pdf)\n"
    );
    drag(cx, "entry:Renamed.pdf", "entry:Archive");
    cx.run_until_parked();
    assert!(folder.path().join("Archive/Renamed.pdf").is_file());
    assert_eq!(
        std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
        "[reference](Archive/Renamed.pdf)\n"
    );
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[0].editor.read(cx).buffer.text(),
            "[reference](Archive/Renamed.pdf)\n"
        )
    });
}
