use super::*;

#[gpui::test]
fn reading_callouts_fold_nested_rows_and_reveal_find_targets(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let source = "> [!note]- Outer\n> Body\n>\n> > [!warning]- Inner\n> > Find me\n\n# After\n";
    std::fs::write(folder.path().join("A.md"), source).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    view.update_in(cx, |this, w, cx| {
        this.set_mode(super::ViewMode::Preview, w, cx)
    });
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(800.)));
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert_eq!(this.tabs[this.active].preview_scroll.item_count(), 2)
    });
    assert!(cx.debug_bounds("fold-callout-2").is_none());
    click(cx, "fold-callout-0");
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].preview_scroll.item_count(),
            4,
            "overrides={:?}",
            this.tabs[this.active].preview_projection.overrides
        );
        assert_eq!(this.tabs[this.active].editor.read(cx).buffer.text(), source);
    });
    click(cx, "fold-callout-2");
    view.read_with(cx, |this, _| {
        assert_eq!(this.tabs[this.active].preview_scroll.item_count(), 5)
    });
    click(cx, "fold-callout-2");
    click(cx, "fold-callout-0");
    view.update(cx, |this, cx| {
        let at = source.find("Find me").unwrap();
        this.find.matches = std::sync::Arc::new(vec![at..at + 7]);
        this.find.selected = Some(0);
        this.reveal_find(cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        assert_eq!(tab.preview_scroll.item_count(), 5);
        assert!(
            tab.prepared
                .callouts
                .iter()
                .all(|c| !tab.preview_projection.collapsed(c))
        );
        assert_eq!(tab.editor.read(cx).buffer.selected(), "Find me");
    });
    // Changed source invalidates source-position fold overrides.
    view.update(cx, |this, cx| {
        this.tabs[this.active].editor.update(cx, |e, cx| {
            e.buffer.replace(0..0, "Preamble\n\n");
            cx.notify();
        });
    });
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(100));
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        assert_eq!(tab.prepared_revision, tab.editor.read(cx).buffer.revision());
        assert!(tab.preview_projection.overrides.is_empty());
        assert_eq!(tab.preview_scroll.item_count(), 3);
    });
}

#[gpui::test]
fn reading_footnotes_reorder_navigate_and_return_without_opening_notes(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let source = format!(
        "[^a]: A **definition**.\n\n# Main\n\nReference[^a] and again[^a].\n\n{}",
        "Body paragraph.\n\n".repeat(80)
    );
    std::fs::write(folder.path().join("A.md"), &source).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    view.update_in(cx, |this, w, cx| {
        this.set_mode(super::ViewMode::Preview, w, cx)
    });
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(800.)));
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        let tab = &this.tabs[this.active];
        assert_eq!(tab.preview_projection.row(0), 1);
        assert!(
            !this
                .link_highlight("opalglass-footnote:1")
                .underline
                .unwrap()
                .wavy
        );
    });
    view.update_in(cx, |this, w, cx| {
        this.follow("opalglass-footnote:1".into(), w, cx)
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("footnote-return-0-1").is_some());
    click(cx, "footnote-return-0-1");
    cx.run_until_parked();
    assert!(cx.debug_bounds("preview-column-2").is_some());
    view.read_with(cx, |this, cx| {
        assert!(this.modal == super::Modal::None);
        assert_eq!(this.tabs.len(), 1);
        assert_eq!(this.tabs[this.active].editor.read(cx).buffer.text(), source);
    });
}

#[gpui::test]
fn custom_reading_tasks_preserve_unicode_markers_on_undo(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let source = "* [?] **Question**\n  + [é] Unicode\n";
    std::fs::write(folder.path().join("A.md"), source).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    view.update_in(cx, |this, w, cx| {
        this.set_mode(super::ViewMode::Preview, w, cx)
    });
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(800.)));
    cx.run_until_parked();
    click(cx, "reading-task-1");
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.text(),
            source.replace("[é]", "[ ]")
        );
    });
    view.update(cx, |this, cx| {
        this.tabs[this.active].editor.update(cx, |e, cx| {
            e.buffer.undo();
            cx.notify();
        })
    });
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[this.active].editor.read(cx).buffer.text(), source)
    });
}

#[gpui::test]
fn reading_tables_and_task_checkboxes_edit_source_with_undo(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let source = "- [ ] **Todo**\n\n| Left | Right |\n| :--- | ---: |\n| One | Two |\n";
    std::fs::write(folder.path().join("A.md"), source).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    view.update_in(cx, |this, w, cx| {
        this.set_mode(super::ViewMode::Preview, w, cx)
    });
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(800.)));
    cx.run_until_parked();
    click(cx, "reading-task-0");
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.text(),
            source.replacen("[ ]", "[x]", 1),
            "status={} revision={} prepared={} task={:?}",
            this.status,
            this.tabs[this.active].editor.read(cx).buffer.revision(),
            this.tabs[this.active].prepared_revision,
            this.tabs[this.active].prepared.rows[0].task
        );
        assert_eq!(
            this.tabs[this.active]
                .prepared
                .rows
                .iter()
                .filter(|r| r.table.is_some())
                .count(),
            2
        );
    });
    view.update(cx, |this, cx| {
        this.tabs[this.active].editor.update(cx, |editor, cx| {
            editor.buffer.undo();
            cx.notify();
        });
    });
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[this.active].editor.read(cx).buffer.text(), source)
    });
}

#[gpui::test]
fn reading_gutters_survive_narrow_split_panes_and_scrolling(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(
        folder.path().join("A.md"),
        "# A heading\n\nA paragraph with enough words to wrap in narrow panes.\n\n".repeat(80),
    )
    .unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    for width in [1600., 880.] {
        cx.simulate_resize(gpui::size(gpui::px(width), gpui::px(700.)));
        for mode in [super::ViewMode::Preview, super::ViewMode::Split] {
            view.update_in(cx, |this, w, cx| {
                this.set_mode(mode, w, cx);
                this.tabs[this.active]
                    .preview_scroll
                    .scroll_to(gpui::ListOffset {
                        item_ix: 0,
                        offset_in_item: gpui::px(0.),
                    });
            });
            cx.run_until_parked();
            for scroll in [false, true] {
                if scroll {
                    let position = cx.debug_bounds("preview-pane").unwrap().center();
                    cx.simulate_event(gpui::ScrollWheelEvent {
                        position,
                        delta: gpui::ScrollDelta::Pixels(gpui::point(
                            gpui::px(0.),
                            gpui::px(-180.),
                        )),
                        ..Default::default()
                    });
                    cx.run_until_parked();
                }
                // Refresh selectors even if the cached preview did not repaint.
                cx.update(|w, _| w.refresh());
                cx.run_until_parked();
                let pane = cx.debug_bounds("preview-pane").unwrap();
                let index = view.read_with(cx, |this, _| {
                    this.tabs[this.active]
                        .preview_scroll
                        .logical_scroll_top()
                        .item_ix
                });
                // GPUI's test selector API requires a static label.
                let selector = Box::leak(format!("preview-column-{index}").into_boxed_str());
                let column = cx.debug_bounds(selector).unwrap();
                assert!(column.left() - pane.left() >= gpui::px(23.));
                assert!(pane.right() - column.right() >= gpui::px(23.));
                assert!(column.size.width <= gpui::px(760.));
                assert!((column.center().x - pane.center().x).abs() < gpui::px(1.));
            }
        }
    }
}

#[gpui::test]
fn syntax_colors_arrive_after_first_paint_and_refresh_after_edits(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "start").unwrap();
    let code = "const café = 42; // comment\n".repeat(2000);
    let source = format!("```js\n{code}```\n");
    std::fs::write(folder.path().join("B.md"), &source).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    let opening_seen = std::rc::Rc::new(std::cell::Cell::new(false));
    let seen = opening_seen.clone();
    let structure_seen = std::rc::Rc::new(std::cell::Cell::new(false));
    let structure = structure_seen.clone();
    view.update_in(cx, |this, w, cx| {
        this._subscriptions
            .push(cx.observe(&cx.entity(), move |this, _, cx| {
                let Some(tab) = this
                    .tabs
                    .get(this.active)
                    .filter(|t| t.path == PathBuf::from("B.md"))
                else {
                    return;
                };
                if !tab.prepared.syntax_ready {
                    seen.set(true);
                    if tab.prepared.complete {
                        assert!(tab.editor.read(cx).first_paint_at.is_some());
                        assert_eq!(tab.prepared.rows[0].code.as_deref(), Some(code.as_str()));
                        structure.set(true);
                    }
                    assert!(tab.prepared.source_syntax.is_empty());
                } else {
                    assert!(tab.editor.read(cx).first_paint_at.is_some());
                }
            }));
        this.open_note("B.md".into(), w, cx);
    });
    cx.run_until_parked();
    assert!(opening_seen.get());
    assert!(structure_seen.get());
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        assert!(tab.prepared.analysis_complete());
        let e = tab.editor.read(cx);
        assert!(!e.source_syntax.is_empty());
        assert_eq!(
            e.source_syntax.as_ref(),
            tab.prepared.source_syntax.as_ref()
        );
    });
    view.update(cx, |this, cx| {
        this.tabs[this.active].editor.update(cx, |e, cx| {
            e.buffer.replace(0..e.buffer.text().len(), "Plain text\n");
            cx.notify();
        });
    });
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(100));
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        assert!(tab.prepared.source_syntax.is_empty());
        assert!(tab.editor.read(cx).source_syntax.is_empty());
        assert_eq!(tab.prepared_revision, tab.editor.read(cx).buffer.revision());
    });
}

#[gpui::test]
fn reading_column_and_complete_code_clipboard(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let code = "let greeting = \"你好\";\n".repeat(100);
    std::fs::write(folder.path().join("A.md"), format!("```rust\n{code}```")).unwrap();
    std::fs::write(folder.path().join("Empty.md"), "").unwrap();
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
    view.update_in(cx, |this, window, cx| {
        this.set_mode(super::ViewMode::Preview, window, cx)
    });
    cx.simulate_resize(gpui::size(gpui::px(1600.), gpui::px(900.)));
    cx.run_until_parked();
    let wide = cx.debug_bounds("preview-column-0").unwrap();
    assert_eq!(wide.size.width, gpui::px(760.));
    let copy = cx.debug_bounds("copy-code-0").unwrap();
    cx.simulate_click(copy.center(), gpui::Modifiers::default());
    view.read_with(cx, |this, cx| {
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), code);
        assert_eq!(this.status, "Code copied to clipboard");
    });
    cx.simulate_resize(gpui::size(gpui::px(880.), gpui::px(560.)));
    cx.run_until_parked();
    let narrow = cx.debug_bounds("preview-column-0").unwrap();
    assert!(narrow.size.width < wide.size.width);
    assert!(narrow.right() <= gpui::px(880.));
    view.update_in(cx, |this, window, cx| {
        this.open_note("Empty.md".into(), window, cx)
    });
    cx.run_until_parked();
    // GPUI's test-only selector map isn't carried over by cached paint.
    // Locate the button in a fresh draw, then exercise its cached handler.
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let start = cx.debug_bounds("empty-source").unwrap();
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    cx.simulate_click(start.center(), gpui::Modifiers::default());
    view.read_with(cx, |this, _| assert!(this.mode == super::ViewMode::Source));
    cx.simulate_input("New thought");
    view.read_with(cx, |this, cx| {
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.text(),
            "New thought"
        )
    });
}

#[gpui::test]
fn live_preview_toggle_works_in_source_split_and_new_tabs(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let text = "# Heading\n\n**bold** with [[Missing|alias]]\n\n```\n**literal**\n```";
    std::fs::write(folder.path().join("A.md"), text).unwrap();
    std::fs::write(folder.path().join("B.md"), text).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    click(cx, "source");
    click(cx, "toggle-live-preview");
    view.read_with(cx, |this, cx| {
        assert!(this.live_preview);
        let tab = &this.tabs[this.active];
        assert!(tab.editor.read(cx).live_preview);
        assert!(!tab.dirty(cx));
    });
    cx.simulate_keystrokes("ctrl-end");
    cx.simulate_input("\nEdited");
    cx.simulate_keystrokes("ctrl-s");
    assert_eq!(
        std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
        format!("{text}\nEdited")
    );
    click(cx, "split");
    click(cx, "lock-reading-scroll");
    view.update_in(cx, |this, w, cx| this.open_note("B.md".into(), w, cx));
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert!(this.tabs[this.active].editor.read(cx).live_preview);
        assert!(
            !this.tabs[this.active]
                .editor
                .read(cx)
                .missing_links
                .is_empty()
        );
    });
    cx.simulate_keystrokes("ctrl-shift-e");
    view.read_with(cx, |this, cx| {
        assert!(!this.live_preview);
        assert!(this.tabs.iter().all(|t| !t.editor.read(cx).live_preview));
        assert!(this.mode == super::ViewMode::Split);
    });
}

#[gpui::test]
fn split_scroll_lock_follows_source_and_unlocks(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let text = (0..300).map(|i| format!("# Section {i}\n\nA **formatted** paragraph with [[Alias|short text]].\n\n\n> A quotation\n\n- item\n\n")).collect::<String>();
    std::fs::write(folder.path().join("A.md"), &text).unwrap();
    std::fs::write(folder.path().join("B.md"), &text).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(800.)));
    cx.run_until_parked();
    let wheel = |cx: &mut gpui::VisualTestContext, pane, delta| {
        let position = cx.debug_bounds(pane).unwrap().center();
        cx.simulate_event(gpui::ScrollWheelEvent {
            position,
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(delta))),
            ..Default::default()
        });
        cx.run_until_parked();
    };
    let assert_aligned = |cx: &gpui::VisualTestContext| {
        view.read_with(cx, |this, cx| {
            let tab = &this.tabs[this.active];
            let (_, byte) = tab.editor.read(cx).scroll_anchor.unwrap();
            let expected = tab
                .prepared
                .rows
                .partition_point(|row| row.source.start as f64 <= byte)
                .saturating_sub(1);
            let actual = tab.preview_scroll.logical_scroll_top();
            // A blank source line can map exactly to the end of a preview row.
            assert!(
                actual.item_ix.abs_diff(expected) <= 1,
                "expected {expected}, got {} at {byte}",
                actual.item_ix
            );
        });
    };
    wheel(cx, "source-pane", -700.);
    view.read_with(cx, |this, _| {
        assert_eq!(
            this.tabs[this.active]
                .preview_scroll
                .logical_scroll_top()
                .item_ix,
            0
        )
    });
    click(cx, "lock-reading-scroll");
    cx.run_until_parked();
    assert_aligned(cx);
    let sidebar_before = view.read_with(cx, |this, cx| this.sidebar.read(cx).render_count);
    for _ in 0..5 {
        wheel(cx, "source-pane", -95.);
        assert_aligned(cx);
    }
    view.read_with(cx, |this, cx| {
        assert_eq!(this.sidebar.read(cx).render_count, sidebar_before)
    });
    let before = view.read_with(cx, |this, cx| {
        this.tabs[this.active]
            .editor
            .read(cx)
            .scroll_anchor
            .unwrap()
            .1
    });
    wheel(cx, "preview-pane", -170.);
    let after = view.read_with(cx, |this, cx| {
        this.tabs[this.active]
            .editor
            .read(cx)
            .scroll_anchor
            .unwrap()
            .1
    });
    assert!(
        after > before,
        "locked reading wheel should scroll the editor too: {before} -> {after}"
    );
    assert_aligned(cx);
    // Keyboard navigation, reflow, and switching tabs must resynchronize too.
    view.update(cx, |this, cx| {
        this.tabs[this.active].editor.update(cx, |editor, cx| {
            let at = editor.buffer.text().find("# Section 90").unwrap();
            editor.jump_to(at, cx);
        })
    });
    cx.run_until_parked();
    assert_aligned(cx);
    cx.simulate_resize(gpui::size(gpui::px(950.), gpui::px(700.)));
    cx.run_until_parked();
    assert_aligned(cx);
    view.update_in(cx, |this, w, cx| this.open_note("B.md".into(), w, cx));
    cx.run_until_parked();
    assert_aligned(cx);
    wheel(cx, "source-pane", -600.);
    assert_aligned(cx);
    click(cx, "source");
    wheel(cx, "source-pane", -400.);
    click(cx, "split");
    cx.run_until_parked();
    assert_aligned(cx);
    // Rebuilding the preview after edits must retain source mapping.
    view.update(cx, |this, cx| {
        this.tabs[this.active].editor.update(cx, |editor, cx| {
            editor.buffer.replace(0..0, "# Inserted\n\n");
            editor.jump_to(0, cx);
        })
    });
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(75));
    cx.run_until_parked();
    wheel(cx, "source-pane", -600.);
    assert_aligned(cx);
    click(cx, "lock-reading-scroll");
    cx.run_until_parked();
    let before = view.read_with(cx, |this, _| {
        this.tabs[this.active].preview_scroll.logical_scroll_top()
    });
    wheel(cx, "source-pane", -250.);
    view.read_with(cx, |this, _| {
        let after = this.tabs[this.active].preview_scroll.logical_scroll_top();
        assert_eq!(after.item_ix, before.item_ix);
        assert_eq!(after.offset_in_item, before.offset_in_item);
    });
    let before = view.read_with(cx, |this, cx| {
        this.tabs[this.active].editor.read(cx).scroll_anchor
    });
    wheel(cx, "preview-pane", -250.);
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[this.active].editor.read(cx).scroll_anchor, before)
    });
    click(cx, "lock-reading-scroll");
    view.update(cx, |this, cx| {
        this.tabs[this.active].editor.update(cx, |editor, cx| {
            editor.jump_to(editor.buffer.text().len(), cx);
        })
    });
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        let tab = &this.tabs[this.active];
        assert!(
            tab.preview_scroll
                .bounds_for_item(tab.prepared.rows.len() - 1)
                .is_some()
        );
    });
}

#[gpui::test]
fn split_divider_resizes_and_preserves_layout(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "# First\nSome text").unwrap();
    std::fs::write(folder.path().join("B.md"), "# Second").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(800.)));
    click(cx, "split");
    let initial = cx.debug_bounds("source-pane").unwrap();
    let divider = cx.debug_bounds("split-resize").unwrap().center();
    // Grab off-center so a press without movement must not jump.
    let grab = divider + gpui::point(gpui::px(2.), gpui::px(0.));
    cx.simulate_mouse_down(grab, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_move(grab, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(
        (f32::from(cx.debug_bounds("source-pane").unwrap().size.width - initial.size.width)).abs()
            < 1.
    );
    let target = grab + gpui::point(gpui::px(120.), gpui::px(0.));
    cx.simulate_mouse_move(target, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_up(target, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let resized = cx.debug_bounds("source-pane").unwrap();
    assert!((f32::from(resized.size.width - initial.size.width) - 120.).abs() < 1.);
    let ratio = view.read_with(cx, |this, _| {
        assert!(this.split_drag_offset.is_none());
        this.split_ratio
    });
    // Mouse movement after release cannot continue resizing.
    cx.simulate_mouse_move(grab, gpui::MouseButton::Left, gpui::Modifiers::default());
    view.read_with(cx, |this, _| assert_eq!(this.split_ratio, ratio));
    click(cx, "reading");
    view.read_with(cx, |this, _| assert!(this.mode == super::ViewMode::Preview));
    click(cx, "source");
    view.read_with(cx, |this, _| assert!(this.mode == super::ViewMode::Source));
    click(cx, "split");
    view.update_in(cx, |this, w, cx| this.open_note("B.md".into(), w, cx));
    cx.run_until_parked();
    assert!(
        (f32::from(cx.debug_bounds("source-pane").unwrap().size.width - resized.size.width)).abs()
            < 1.
    );
    // Geometry must follow sidebar and window changes, not a fixed origin.
    cx.simulate_keystrokes("ctrl-b");
    cx.simulate_resize(gpui::size(gpui::px(1000.), gpui::px(700.)));
    cx.run_until_parked();
    let bounds = cx.debug_bounds("note-panes").unwrap();
    let source = cx.debug_bounds("source-pane").unwrap();
    assert!(
        (f32::from(source.size.width)
            / (f32::from(bounds.size.width) - super::SPLIT_DIVIDER_WIDTH)
            - ratio)
            .abs()
            < 0.005
    );
    // Dragging to the edge still leaves a usable reading pane.
    let from = cx.debug_bounds("split-resize").unwrap().center();
    let to = gpui::point(bounds.right() - gpui::px(1.), from.y);
    cx.simulate_mouse_down(from, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_move(to, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_up(to, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(cx.debug_bounds("preview-pane").unwrap().size.width >= gpui::px(159.));
    // Icon actions still use the existing confirmation dialogs.
    click(cx, "copy");
    view.read_with(cx, |this, _| assert!(this.modal == super::Modal::Copy));
    cx.simulate_keystrokes("escape");
    click(cx, "reload");
    view.read_with(cx, |this, _| assert!(this.modal == super::Modal::Reload));
}

#[gpui::test]
fn preview_and_dirty_state_track_edits_undo_and_async_heading_navigation(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "# First\n\n#old").unwrap();
    std::fs::write(
        folder.path().join("B.md"),
        format!("{}\n# Destination\n", "paragraph\n\n".repeat(4000)),
    )
    .unwrap();
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("#new");
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(75));
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[0].prepared.tags, vec!["new"]);
        assert!(this.tabs[0].dirty(cx));
    });
    cx.simulate_keystrokes("ctrl-z ctrl-z ctrl-z ctrl-z");
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(75));
    cx.run_until_parked();
    view.read_with(cx, |this, cx| assert!(!this.tabs[0].dirty(cx)));
    view.update_in(cx, |this, window, cx| {
        this.open_note_at("B.md".into(), Some("Destination".into()), window, cx)
    });
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        let editor = tab.editor.read(cx);
        assert_eq!(
            &editor.buffer.text()[editor.buffer.cursor()..],
            "# Destination\n"
        );
        assert!(
            tab.preview_scroll
                .bounds_for_item(tab.prepared.headings[0].1)
                .is_some()
        );
    });
    view.update(cx, |this, cx| {
        this.tabs[this.active]
            .preview_scroll
            .scroll_to(gpui::ListOffset {
                item_ix: 0,
                offset_in_item: gpui::px(0.),
            });
        this.preview_pane.update(cx, |_, cx| cx.notify());
    });
    cx.run_until_parked();
    view.update_in(cx, |this, window, cx| {
        this.open_note_at("B.md".into(), Some("Destination".into()), window, cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        let tab = &this.tabs[this.active];
        assert!(
            tab.preview_scroll
                .bounds_for_item(tab.prepared.headings[0].1)
                .is_some()
        );
    });
}

#[gpui::test]
fn incomplete_code_preview_never_copies_a_truncated_block(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let code = "let greeting = \"你好\";\n".repeat(2000);
    let source = format!("```rust\n{code}```");
    std::fs::write(folder.path().join("A.md"), &source).unwrap();
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
    // Hold the opening stage to exercise its controls before completion.
    view.update(cx, |this, cx| {
        let tab = &mut this.tabs[this.active];
        tab.prepared = std::sync::Arc::new(opalglass::prepared::Prepared::opening(&source));
        tab.initial_frame_pending = false;
        let count = tab.prepared.rows.len();
        tab.preview_scroll.reset(count);
        cx.notify();
    });
    cx.run_until_parked();
    click(cx, "copy-code-0");
    view.read_with(cx, |this, cx| {
        assert_eq!(this.status, "This code block is still loading.");
        assert!(cx.read_from_clipboard().is_none());
    });
    view.update(cx, |this, cx| {
        this.tabs[this.active].requested_revision = u64::MAX;
        this.update_document(cx);
    });
    cx.run_until_parked();
    click(cx, "copy-code-0");
    view.read_with(cx, |this, cx| {
        assert!(this.tabs[this.active].prepared.complete);
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), code);
    });
}

#[gpui::test]
fn reading_mode_keeps_workspace_shortcuts_available(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("Note.md"), "# Note").unwrap();
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
    cx.simulate_keystrokes("ctrl-e ctrl-n");
    view.read_with(cx, |this, _| {
        assert!(this.mode == super::ViewMode::Preview);
        assert!(this.modal == super::Modal::New);
    });
    cx.simulate_keystrokes("escape ctrl-e");
    view.read_with(cx, |this, _| assert!(this.mode == super::ViewMode::Source));
}
