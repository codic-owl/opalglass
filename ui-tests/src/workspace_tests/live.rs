use super::*;

#[gpui::test]
fn editing_preview_context_arrives_and_refreshes_after_definition_edits(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let text = "# A\n\n[Label][ref] and note[^n].\n\n[ref]: B.md\n[^n]: Body\n";
    std::fs::write(folder.path().join("A.md"), text).unwrap();
    std::fs::write(folder.path().join("B.md"), "B").unwrap();
    std::fs::write(folder.path().join("C.md"), "C").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    view.update_in(cx, |this, w, cx| {
        this.open_note("A.md".into(), w, cx);
        if !this.live_preview {
            this.toggle_live_preview(w, cx);
        }
    });
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        let editor = this.tabs[this.active].editor.read(cx);
        assert_eq!(
            editor
                .link_target_at(text.find("Label").unwrap())
                .as_deref(),
            Some("B.md")
        );
        assert_eq!(
            editor.link_target_at(text.find("[^n]").unwrap()).as_deref(),
            Some("opalglass-footnote:1")
        );
    });
    view.update(cx, |this, cx| {
        this.tabs[this.active].editor.update(cx, |e, cx| {
            let at = e.buffer.text().find("B.md").unwrap();
            e.buffer.replace(at..at + 4, "C.md");
            cx.notify();
        })
    });
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(100));
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        let editor = this.tabs[this.active].editor.read(cx);
        assert_eq!(
            editor
                .link_target_at(text.find("Label").unwrap())
                .as_deref(),
            Some("C.md")
        );
        assert_eq!(editor.buffer.text(), text.replace("B.md", "C.md"));
    });
    view.update_in(cx, |this, w, cx| this.focus_document(w, cx));
    cx.simulate_keystrokes("ctrl-z");
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(100));
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        let editor = this.tabs[this.active].editor.read(cx);
        assert_eq!(editor.buffer.text(), text);
        assert_eq!(
            editor
                .link_target_at(text.find("Label").unwrap())
                .as_deref(),
            Some("B.md")
        );
    });
}

fn mixed_preview_note() -> String {
    let mut text = (0..90).map(|i| format!(
        "# Section {i}\n\nVisible **é 👩‍💻** [reference][target] and note[^n].\n\n%% Hidden {i}\n{}%%\n\n> [!note]- Outer {i}\n> Outer body\n>\n> > [!tip]- Inner {i}\n> > ## Hidden {i}\n> > Body **Ω 👩‍💻** and [reference][target].\n>\n> Outer tail\n\nAfter {i} **é**.\n\n",
        "comment α\n".repeat(8)
    )).collect::<String>();
    text.push_str("[target]: B.md\n\n[^n]: Footnote **é 👩‍💻** with [reference][target].\n");
    text
}

fn wheel_preview_pane(cx: &mut gpui::VisualTestContext, pane: &'static str, delta: f32) {
    let position = cx.debug_bounds(pane).unwrap().center();
    cx.simulate_event(gpui::ScrollWheelEvent {
        position,
        delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(delta))),
        ..Default::default()
    });
    cx.run_until_parked();
}

fn assert_mixed_split_alignment(
    view: &gpui::Entity<Workspace>,
    cx: &gpui::VisualTestContext,
    physical_rows: &[std::ops::Range<usize>],
) {
    view.read_with(cx, |this, cx| {
        assert!(this.mode == ViewMode::Split && this.scroll_locked);
        let tab = &this.tabs[this.active];
        let editor = tab.editor.read(cx);
        let (revision, byte) = editor.scroll_anchor.unwrap();
        assert_eq!(revision, editor.buffer.revision());
        let physical = &physical_rows[editor.saved_scroll().row];
        assert!(
            physical.start as f64 <= byte && byte <= physical.end as f64,
            "published source anchor {byte} must belong to physical row {physical:?}"
        );
        let row = tab.prepared.row_at_source(byte as usize);
        let expected = tab.preview_projection.list_index(row);
        let actual = tab.preview_scroll.logical_scroll_top();
        // An anchor at the end of a source row can normalize to its next item.
        assert!(
            actual.item_ix.abs_diff(expected) <= 1,
            "expected Reading item {expected}, got {} for physical byte {byte}",
            actual.item_ix
        );
    });
}

fn assert_unlocked_split_independence(
    view: &gpui::Entity<Workspace>,
    cx: &mut gpui::VisualTestContext,
) {
    view.read_with(cx, |this, _| {
        assert!(this.mode == ViewMode::Split && !this.scroll_locked)
    });
    let reading_before = view.read_with(cx, |this, _| {
        this.tabs[this.active].preview_scroll.logical_scroll_top()
    });
    wheel_preview_pane(cx, "source-pane", -220.);
    view.read_with(cx, |this, _| {
        let after = this.tabs[this.active].preview_scroll.logical_scroll_top();
        assert_eq!(after.item_ix, reading_before.item_ix);
        assert_eq!(after.offset_in_item, reading_before.offset_in_item);
    });
    let source_before = view.read_with(cx, |this, cx| {
        this.tabs[this.active].editor.read(cx).scroll_anchor
    });
    wheel_preview_pane(cx, "preview-pane", -220.);
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).scroll_anchor,
            source_before
        );
    });
}

#[gpui::test]
fn mixed_editing_preview_keeps_split_scroll_and_mode_transitions_on_source_anchors(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    let text = mixed_preview_note();
    let physical_rows = opalglass::document::source_rows(&text);
    std::fs::write(folder.path().join("A.md"), &text).unwrap();
    std::fs::write(folder.path().join("B.md"), "Target").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.simulate_resize(gpui::size(gpui::px(1280.), gpui::px(800.)));
    cx.run_until_parked();
    view.update_in(cx, |this, w, cx| {
        if !this.live_preview {
            this.toggle_live_preview(w, cx);
        }
        this.set_mode(ViewMode::Split, w, cx);
    });
    cx.run_until_parked();
    click(cx, "lock-reading-scroll");
    let section = text.find("# Section 20\n").unwrap();
    view.update(cx, |this, cx| {
        this.tabs[this.active]
            .editor
            .update(cx, |editor, cx| editor.jump_to(section, cx));
    });
    cx.run_until_parked();
    assert_mixed_split_alignment(&view, cx, &physical_rows);
    let physical_section = physical_rows.partition_point(|row| row.start <= section) - 1;
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).saved_scroll().row,
            physical_section
        );
        assert!(this.tabs[this.active].prepared.complete);
    });

    // Changing source presentation preserves a physical row despite removing
    // hundreds of comment lines and folded callout bodies from the visible list.
    for live in [false, true] {
        let before = view.read_with(cx, |this, cx| {
            this.tabs[this.active].editor.read(cx).saved_scroll()
        });
        click(cx, "toggle-live-preview");
        cx.run_until_parked();
        view.read_with(cx, |this, cx| {
            assert_eq!(this.live_preview, live);
            assert_eq!(
                this.tabs[this.active].editor.read(cx).saved_scroll().row,
                before.row
            );
        });
        assert_mixed_split_alignment(&view, cx, &physical_rows);
    }
    for _ in 0..4 {
        wheel_preview_pane(cx, "source-pane", -140.);
        assert_mixed_split_alignment(&view, cx, &physical_rows);
    }

    click(cx, "source");
    wheel_preview_pane(cx, "source-pane", -220.);
    let source_before = view.read_with(cx, |this, cx| {
        this.tabs[this.active].editor.read(cx).scroll_anchor
    });
    click(cx, "reading");
    wheel_preview_pane(cx, "preview-pane", -170.);
    view.read_with(cx, |this, cx| {
        assert!(this.mode == ViewMode::Preview);
        assert_eq!(
            this.tabs[this.active].editor.read(cx).scroll_anchor,
            source_before
        );
    });
    click(cx, "split");
    cx.run_until_parked();
    assert_mixed_split_alignment(&view, cx, &physical_rows);

    // A heading jump into two collapsed containers must reveal both panes and
    // publish the target's physical source position, not its compact list index.
    view.update_in(cx, |this, w, cx| this.follow("#Hidden 60".into(), w, cx));
    cx.run_until_parked();
    let target = text.find("## Hidden 60\n").unwrap();
    let physical_target = physical_rows.partition_point(|row| row.start <= target) - 1;
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        let editor = tab.editor.read(cx);
        assert_eq!(editor.buffer.cursor(), target);
        assert_eq!(editor.saved_scroll().row, physical_target);
        let ancestors = tab.prepared.callouts.iter()
            .filter(|c| c.source.contains(&target)).collect::<Vec<_>>();
        assert_eq!(ancestors.len(), 2);
        assert!(ancestors.iter().all(|c| !tab.preview_projection.collapsed(c)));
        assert!(
            tab.preview_scroll
                .bounds_for_item(
                    tab.preview_projection
                        .list_index(tab.prepared.row_at_source(target))
                )
                .is_some(),
            "target row {} / item {} is unmeasured; Reading at {:?}, source anchor {:?}, physical {}, preparedrev {}, locked {}",
            tab.prepared.row_at_source(target),
            tab.preview_projection.list_index(tab.prepared.row_at_source(target)),
            tab.preview_scroll.logical_scroll_top(),
            editor.scroll_anchor,
            editor.saved_scroll().row,
            tab.prepared_revision,
            this.scroll_locked,
        );
        assert_eq!(editor.buffer.text(), text);
    });
    assert_mixed_split_alignment(&view, cx, &physical_rows);

    click(cx, "lock-reading-scroll");
    cx.run_until_parked();
    assert_unlocked_split_independence(&view, cx);
}
