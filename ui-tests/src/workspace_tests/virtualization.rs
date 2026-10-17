use super::*;

#[gpui::test]
fn scrolling_only_renders_the_changed_pane_for_short_and_long_notes(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    for (name, lines) in [("A", 50), ("B", 20_000)] {
        std::fs::write(
            folder.path().join(format!("{name}.md")),
            "A paragraph with [[Missing]].\n\n".repeat(lines),
        )
        .unwrap();
    }
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
    for name in ["A.md", "B.md"] {
        view.update_in(cx, |this, window, cx| {
            this.open_note(name.into(), window, cx)
        });
        cx.run_until_parked();
        let counts = |cx: &gpui::VisualTestContext| {
            view.read_with(cx, |this, cx| {
                (
                    this.tabs[this.active].editor.read(cx).render_count,
                    this.preview_pane.read(cx).render_count,
                    this.sidebar.read(cx).render_count,
                    this.connections.read(cx).render_count,
                )
            })
        };
        for pane in ["source-pane", "preview-pane"] {
            let before = counts(cx);
            let position = cx.debug_bounds(pane).unwrap().center();
            for _ in 0..5 {
                cx.simulate_event(gpui::ScrollWheelEvent {
                    position,
                    delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-60.))),
                    ..Default::default()
                });
                cx.run_until_parked();
            }
            let after = counts(cx);
            if pane == "source-pane" {
                assert!(after.0 > before.0);
                assert_eq!(after.1, before.1, "source scroll redrew preview");
            } else {
                assert_eq!(after.0, before.0, "preview scroll redrew editor");
                assert!(after.1 > before.1);
                view.read_with(cx, |this, _| {
                    assert!(
                        this.tabs[this.active]
                            .preview_scroll
                            .logical_scroll_top()
                            .item_ix
                            > 0
                    )
                });
            }
            assert_eq!(after.2, before.2, "scroll redrew explorer");
            assert_eq!(after.3, before.3, "scroll redrew connections");
            view.read_with(cx, |this, cx| {
                assert_eq!(this.tabs[this.active].editor.read(cx).buffer.cursor(), 0);
                assert!(!this.tabs[this.active].dirty(cx));
            });
        }
    }
}

#[gpui::test]
fn connections_are_virtualized_and_last_backlink_and_tag_are_clickable(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "#topic").unwrap();
    for i in 0..400 {
        std::fs::write(folder.path().join(format!("Incoming{i:04}.md")), "[[A]]").unwrap();
    }
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
    view.read_with(cx, |this, cx| {
        let panel = this.connections.read(cx);
        assert_eq!(panel.backlinks, 400);
        assert_eq!(panel.tags, 1);
    });
    assert!(cx.debug_bounds("connection-0").is_some());
    assert!(cx.debug_bounds("connection-399").is_none());
    view.update(cx, |this, cx| {
        this.connections.update(cx, |panel, cx| {
            panel.scroll.scroll_to_item(
                panel.items.len().div_ceil(3) - 1,
                gpui::ScrollStrategy::Bottom,
            );
            cx.notify();
        });
    });
    cx.run_until_parked();
    click(cx, "connection-400");
    view.read_with(cx, |this, cx| {
        assert_eq!(this.search.read(cx).buffer.text(), "tag:topic")
    });
    click(cx, "connection-399");
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].path,
            PathBuf::from("Incoming0399.md")
        );
        assert_eq!(this.connections.read(cx).backlinks, 0);
    });
}

#[gpui::test]
fn first_opening_frame_precedes_full_analysis_and_preserves_immediate_edits(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "start").unwrap();
    let original = format!(
        "# First\n\n{}\n# Last\n\n[[Missing]]",
        "paragraph\n\n".repeat(4000)
    );
    std::fs::write(folder.path().join("B.md"), &original).unwrap();
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
    let initial_seen = std::rc::Rc::new(std::cell::Cell::new(false));
    let completed_seen = std::rc::Rc::new(std::cell::Cell::new(false));
    let initial = initial_seen.clone();
    let completed = completed_seen.clone();
    let expected = original.clone();
    view.update_in(cx, |this, window, cx| {
        this._subscriptions
            .push(cx.observe(&cx.entity(), move |this, _, cx| {
                let Some(tab) = this
                    .tabs
                    .get(this.active)
                    .filter(|t| t.path == PathBuf::from("B.md"))
                else {
                    return;
                };
                if !tab.prepared.complete && !initial.replace(true) {
                    assert!(tab.initial_frame_pending);
                    assert!(tab.editor.read(cx).first_paint_at.is_none());
                    assert_eq!(tab.editor.read(cx).buffer.text(), expected);
                    assert_eq!(tab.prepared.rows[0].text.as_ref(), "First");
                    assert!(tab.prepared.rows.len() < 1000);
                    // Type while only the opening preview is available.
                    tab.editor.update(cx, |editor, cx| {
                        editor.buffer.move_to(editor.buffer.text().len(), false);
                        editor.buffer.insert("\n\n#typed");
                        cx.notify();
                    });
                } else if tab.prepared.complete {
                    assert!(tab.editor.read(cx).first_paint_at.is_some());
                    completed.set(true);
                }
            }));
        this.open_note("B.md".into(), window, cx);
    });
    cx.run_until_parked();
    assert!(initial_seen.get());
    assert!(completed_seen.get());
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        assert_eq!(
            tab.editor.read(cx).buffer.text(),
            format!("{original}\n\n#typed")
        );
        assert_eq!(tab.baseline, original);
        assert!(tab.dirty(cx));
        assert_eq!(tab.prepared_revision, tab.editor.read(cx).buffer.revision());
        assert_eq!(tab.prepared.tags, vec!["typed"]);
        assert!(!tab.editor.read(cx).missing_links.is_empty());
    });
}
