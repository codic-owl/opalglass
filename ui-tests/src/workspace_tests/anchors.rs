use super::*;

#[gpui::test]
fn section_and_block_links_navigate_both_panes_and_expand_callouts(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let source = "# Top\n\n[[#Setext]] [[#^block]] [[#Missing]] [missing](#Absent)\n\nSetext\n------\n\nParagraph é ^block\n\n> [!note]- Collapsed\n> ## Hidden **heading**\n> Inside\n\n# End\n";
    std::fs::write(folder.path().join("A.md"), source).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    for (fragment, needle) in [
        ("Setext", "Setext\n---"),
        ("^block", "Paragraph é"),
        ("Hidden heading", "## Hidden"),
    ] {
        view.update_in(cx, |this, w, cx| this.follow(format!("#{fragment}"), w, cx));
        cx.run_until_parked();
        view.read_with(cx, |this, cx| {
            let tab = &this.tabs[this.active];
            let byte = source.find(needle).unwrap();
            assert_eq!(tab.editor.read(cx).buffer.cursor(), byte, "{fragment}");
            let row = tab.prepared.row_at_source(byte);
            assert!(
                tab.preview_scroll
                    .bounds_for_item(tab.preview_projection.list_index(row))
                    .is_some(),
                "target row must be visible"
            );
            assert!(
                !this
                    .link_highlight(&format!("#{fragment}"))
                    .underline
                    .unwrap()
                    .wavy
            );
        });
    }
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        assert!(
            tab.prepared
                .callouts
                .iter()
                .all(|c| !tab.preview_projection.collapsed(c))
        );
        assert_eq!(tab.editor.read(cx).missing_links.len(), 2);
        assert!(this.link_highlight("#Missing").underline.unwrap().wavy);
        assert!(this.link_highlight("#Absent").underline.unwrap().wavy);
    });
    view.update_in(cx, |this, w, cx| this.follow("#^absent".into(), w, cx));
    view.read_with(cx, |this, _| {
        assert!(matches!(this.modal, Modal::None));
        assert!(this.status.contains("not found"));
    });
}

#[gpui::test]
fn anchors_beyond_opening_prefix_wait_for_analysis_and_respect_user_navigation(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    let source = format!(
        "# Top\n\n{}\nLate heading\n============\n\nLast paragraph ^last\n",
        "padding paragraph\n\n".repeat(900)
    );
    std::fs::write(folder.path().join("A.md"), &source).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    for fragment in ["Late heading", "^last"] {
        view.update_in(cx, |this, w, cx| {
            let tab = &mut this.tabs[this.active];
            tab.install_prepared(
                std::sync::Arc::new(opalglass::prepared::Prepared::opening(&source)),
                false,
            );
            tab.requested_revision = u64::MAX;
            this.finish_note_navigation(Some(fragment.into()), w, cx);
            assert!(this.tabs[this.active].pending_anchor.is_some());
        });
        cx.run_until_parked();
        view.read_with(cx, |this, cx| {
            let tab = &this.tabs[this.active];
            assert!(tab.prepared.complete);
            assert!(tab.pending_anchor.is_none());
            let byte = tab.prepared.anchors.target(fragment).unwrap().start;
            assert_eq!(tab.editor.read(cx).buffer.cursor(), byte);
            assert!(
                tab.preview_scroll
                    .bounds_for_item(
                        tab.preview_projection
                            .list_index(tab.prepared.row_at_source(byte))
                    )
                    .is_some(),
                "late target must be visible"
            );
        });
    }
    view.update_in(cx, |this, w, cx| {
        let tab = &mut this.tabs[this.active];
        tab.install_prepared(
            std::sync::Arc::new(opalglass::prepared::Prepared::opening(&source)),
            false,
        );
        tab.requested_revision = u64::MAX;
        this.finish_note_navigation(Some("^last".into()), w, cx);
        this.tabs[this.active]
            .editor
            .update(cx, |e, cx| e.jump_to(2, cx));
    });
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[this.active].editor.read(cx).buffer.cursor(), 2)
    });
}

#[gpui::test]
fn missing_sections_refresh_after_saved_and_unsaved_target_changes(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "[[B#Later]] [[B#^later]]\n").unwrap();
    std::fs::write(folder.path().join("B.md"), "# Existing\n").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    view.update_in(cx, |this, w, cx| this.open_note("A.md".into(), w, cx));
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).missing_links.len(),
            2
        )
    });
    std::fs::write(folder.path().join("B.md"), "Later\n=====\n\nText ^later\n").unwrap();
    view.update(cx, |this, cx| {
        this.apply_scan(
            opalglass::vault::Vault::open(folder.path()).unwrap(),
            this.revision,
            cx,
        )
    });
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert!(
            this.tabs[this.active]
                .editor
                .read(cx)
                .missing_links
                .is_empty()
        )
    });
    view.update_in(cx, |this, w, cx| this.follow("B#^later".into(), w, cx));
    cx.run_until_parked();
    view.update(cx, |this, cx| {
        this.tabs[this.active]
            .editor
            .update(cx, |e, cx| e.set_text("# Replaced\n", cx))
    });
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(100));
    cx.run_until_parked();
    view.update_in(cx, |this, w, cx| this.open_note("A.md".into(), w, cx));
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).missing_links.len(),
            2
        )
    });
}
