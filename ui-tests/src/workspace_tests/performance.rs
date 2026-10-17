use super::*;

#[cfg(debug_assertions)]
#[gpui::test]
fn performance_bar_logs_actions_once_and_copies_bounded_history(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    for name in ["A", "B"] {
        std::fs::write(folder.path().join(format!("{name}.md")), name).unwrap();
    }
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    let latest = |cx: &gpui::VisualTestContext| {
        view.read_with(cx, |this, cx| {
            let entry = this.performance.read(cx).history.back().unwrap();
            (entry.action, entry.target.clone())
        })
    };
    assert_eq!(latest(cx), ("Open note", "A.md".into()));
    view.update_in(cx, |this, w, cx| this.open_note("B.md".into(), w, cx));
    cx.run_until_parked();
    assert_eq!(latest(cx), ("Open note", "B.md".into()));
    let before = view.read_with(cx, |this, cx| this.performance.read(cx).history.len());
    click(cx, "tab-0");
    cx.run_until_parked();
    assert_eq!(latest(cx), ("Switch tab", "A.md".into()));
    view.read_with(cx, |this, cx| {
        assert_eq!(this.performance.read(cx).history.len(), before + 1)
    });
    cx.simulate_input("mouse ");
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[0].editor.read(cx).buffer.text(), "mouse A")
    });
    cx.simulate_keystrokes("ctrl-tab");
    assert_eq!(latest(cx), ("Switch tab", "B.md".into()));
    cx.simulate_input("edited ");
    cx.simulate_keystrokes("ctrl-w");
    assert_eq!(latest(cx), ("Close tab", "B.md".into()));
    view.read_with(cx, |this, cx| {
        assert!(
            this.performance
                .read(cx)
                .history
                .iter()
                .any(|e| e.action == "Save note")
        )
    });
    for _ in 0..25 {
        view.update_in(cx, |this, w, cx| this.open_note("A.md".into(), w, cx));
        cx.run_until_parked();
    }
    assert_eq!(latest(cx), ("Tab already open", "A.md".into()));
    view.read_with(cx, |this, cx| {
        assert_eq!(this.performance.read(cx).history.len(), 20)
    });
    click(cx, "performance-log");
    view.read_with(cx, |this, cx| {
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().unwrap(),
            this.performance.read(cx).log_text()
        );
    });
    // The measurement spans through paint, not just the synchronous handler.
    view.update(cx, |this, cx| {
        this.log_performance(
            "Test",
            "paint boundary".into(),
            std::time::Instant::now() - std::time::Duration::from_millis(50),
            cx,
        );
        assert_ne!(
            this.performance.read(cx).history.back().unwrap().action,
            "Test"
        );
    });
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        let entry = this.performance.read(cx).history.back().unwrap();
        assert_eq!(entry.action, "Test");
        assert!(entry.elapsed >= std::time::Duration::from_millis(50));
    });
    // A conflicting close is measured as blocked, never as successful.
    cx.simulate_input("local ");
    std::fs::write(folder.path().join("A.md"), "external").unwrap();
    cx.simulate_keystrokes("ctrl-w");
    assert_eq!(latest(cx), ("Close blocked", "A.md".into()));
    view.read_with(cx, |this, _| assert_eq!(this.tabs.len(), 1));
}

#[gpui::test]
#[ignore]
fn profile_tab_work(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let text = std::env::var_os("OPALGLASS_PROFILE_NOTE")
        .map(|p| std::fs::read_to_string(p).unwrap())
        .unwrap_or_else(|| "paragraph with [[A]] and #tag.\n\n".repeat(8000));
    for name in ["A", "B", "C"] {
        std::fs::write(folder.path().join(format!("{name}.md")), &text).unwrap();
    }
    let incoming = std::env::var("OPALGLASS_PROFILE_BACKLINKS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    for i in 0..incoming {
        std::fs::write(
            folder.path().join(format!("Incoming{i:04}.md")),
            "[[A]] [[B]] [[C]]",
        )
        .unwrap();
    }
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    for path in ["B.md", "C.md"] {
        view.update_in(cx, |this, w, cx| this.open_note(path.into(), w, cx));
        cx.run_until_parked();
        cx.simulate_keystrokes("ctrl-end");
    }
    for mode in [
        super::ViewMode::Source,
        super::ViewMode::Split,
        super::ViewMode::Preview,
    ] {
        view.update_in(cx, |this, w, cx| this.set_mode(mode, w, cx));
        cx.run_until_parked();
        let start = std::time::Instant::now();
        for i in 0..30 {
            view.update_in(cx, |this, w, cx| this.activate_tab(i % 3, w, cx));
            cx.run_until_parked();
        }
        println!(
            "30 existing tab switches, mode {}: {:?}",
            mode as u8,
            start.elapsed()
        );
        let start = std::time::Instant::now();
        for _ in 0..10 {
            view.update_in(cx, |this, w, cx| this.open_note("C.md".into(), w, cx));
            cx.run_until_parked();
        }
        println!(
            "10 already-active opens, mode {}: {:?}",
            mode as u8,
            start.elapsed()
        );
    }
    let start = std::time::Instant::now();
    view.update_in(cx, |this, w, cx| this.close_tab_at(1, w, cx));
    cx.run_until_parked();
    println!("close clean background tab: {:?}", start.elapsed());
    view.update_in(cx, |this, w, cx| {
        this.set_mode(super::ViewMode::Source, w, cx)
    });
    cx.simulate_input("edited");
    let start = std::time::Instant::now();
    view.update_in(cx, |this, w, cx| this.close_tab(w, cx));
    cx.run_until_parked();
    println!("save and close active tab: {:?}", start.elapsed());
}

#[gpui::test]
#[ignore]
fn profile_note_work(cx: &mut TestAppContext) {
    let text = std::env::var_os("OPALGLASS_PROFILE_NOTE")
        .map(|path| std::fs::read_to_string(path).unwrap())
        .unwrap_or_else(|| {
            "## Heading\n\nA paragraph with **bold** and [[Start]] and #tag.\n\n".repeat(4000)
        });
    let prep_start = std::time::Instant::now();
    let normalized = opalglass::document::normalize(&text);
    let buffer = opalglass::buffer::Buffer::new(normalized);
    println!("normalize + buffer: {:?}", prep_start.elapsed());
    let prep_start = std::time::Instant::now();
    let _rows = opalglass::document::source_rows(buffer.text());
    println!("source row indexing: {:?}", prep_start.elapsed());
    let prep_start = std::time::Instant::now();
    let _initial = opalglass::prepared::Prepared::opening(buffer.text());
    println!("opening preview: {:?}", prep_start.elapsed());
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("Start.md"), "start").unwrap();
    std::fs::write(folder.path().join("Target.md"), &text).unwrap();
    let incoming = std::env::var("OPALGLASS_PROFILE_BACKLINKS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    for i in 0..incoming {
        std::fs::write(
            folder.path().join(format!("Incoming{i:04}.md")),
            "[[Target]]",
        )
        .unwrap();
    }
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
    let start = std::time::Instant::now();
    view.update_in(cx, |this, window, cx| {
        this.open_note("Target.md".into(), window, cx)
    });
    cx.run_until_parked();
    let first_paint = view.read_with(cx, |this, cx| {
        assert!(this.tabs[this.active].prepared.complete);
        this.tabs[this.active]
            .editor
            .read(cx)
            .first_paint_at
            .unwrap()
            .duration_since(start)
    });
    println!(
        "note bytes: {}; first text paint: {:?}; full preparation: {:?}",
        text.len(),
        first_paint,
        start.elapsed()
    );
    let start = std::time::Instant::now();
    for _ in 0..3 {
        view.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
    }
    println!("3 note redraws: {:?}", start.elapsed());
    for mode in [
        super::ViewMode::Source,
        super::ViewMode::Preview,
        super::ViewMode::Split,
    ] {
        view.update_in(cx, |this, window, cx| this.set_mode(mode, window, cx));
        cx.run_until_parked();
        for pane in ["source-pane", "preview-pane"] {
            if (pane == "source-pane" && mode == super::ViewMode::Preview)
                || (pane == "preview-pane" && mode == super::ViewMode::Source)
            {
                continue;
            }
            let Some(bounds) = cx.debug_bounds(pane) else {
                continue;
            };
            let start = std::time::Instant::now();
            for _ in 0..60 {
                cx.simulate_event(gpui::ScrollWheelEvent {
                    position: bounds.center(),
                    delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-40.))),
                    ..Default::default()
                });
                cx.run_until_parked();
            }
            println!(
                "60 scroll frames (mode {}, {pane}): {:?}",
                mode as u8,
                start.elapsed()
            );
        }
    }
    let start = std::time::Instant::now();
    cx.simulate_keystrokes("ctrl-end");
    cx.simulate_input("x");
    println!("jump to end + insert: {:?}", start.elapsed());
    let start = std::time::Instant::now();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(75));
    cx.run_until_parked();
    println!("background preview update: {:?}", start.elapsed());
    let start = std::time::Instant::now();
    for path in ["Start.md", "Target.md", "Start.md", "Target.md"] {
        view.update_in(cx, |this, window, cx| {
            this.open_note(path.into(), window, cx)
        });
        cx.run_until_parked();
    }
    println!("4 cached tab switches: {:?}", start.elapsed());
    let start = std::time::Instant::now();
    cx.simulate_keystrokes("ctrl-s");
    println!("save temporary copy: {:?}", start.elapsed());
}

// Explicit opt-in: only opens and redraws the vault, never edits or saves.
#[gpui::test]
#[ignore]
fn profile_vault_redraws(cx: &mut TestAppContext) {
    let root = std::env::var_os("OPALGLASS_PROFILE_VAULT").expect("set OPALGLASS_PROFILE_VAULT");
    let start = std::time::Instant::now();
    let milestones = std::rc::Rc::new(std::cell::RefCell::new([None; 3]));
    let recorded = milestones.clone();
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::new(Some(root.into()), window, cx);
        workspace
            ._subscriptions
            .push(cx.observe(&cx.entity(), move |this, _, _| {
                let mut times = recorded.borrow_mut();
                if this.vault.is_some() {
                    times[0].get_or_insert_with(|| start.elapsed());
                }
                if !this.tabs.is_empty() {
                    times[1].get_or_insert_with(|| start.elapsed());
                }
                if this.vault.as_ref().is_some_and(|v| v.content_indexed()) {
                    times[2].get_or_insert_with(|| start.elapsed());
                }
            }));
        workspace
    });
    let count = view.read_with(cx, |this, _| this.vault.as_ref().unwrap().notes.len());
    println!(
        "simulated UI startup [vault available, first note, indexed]: {:?}; notes: {count}",
        milestones.borrow()
    );
    let start = std::time::Instant::now();
    for _ in 0..10 {
        view.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
    }
    println!("10 simulated UI redraws: {:?}", start.elapsed());
    if count > 100 {
        let start = std::time::Instant::now();
        view.update(cx, |this, cx| {
            this.navigation.expanded = this
                .vault
                .as_ref()
                .unwrap()
                .folders
                .iter()
                .cloned()
                .collect();
            this.tree.flatten(&this.navigation.expanded);
            let count = this.tree.visible.len();
            this.notes_scroll
                .scroll_to_item(count - 1, gpui::ScrollStrategy::Bottom);
            cx.notify();
        });
        cx.run_until_parked();
        println!(
            "expand every folder + scroll to last entry: {:?}",
            start.elapsed()
        );
        view.read_with(cx, |this, _| {
            let scroll = this.notes_scroll.0.borrow();
            let size = scroll.last_item_size.unwrap();
            let bottom_offset = size.contents.height - size.item.height;
            assert!(bottom_offset > gpui::px(0.));
            assert!((-scroll.base_handle.offset().y - bottom_offset).abs() < gpui::px(1.));
        });
    }
}
