use super::*;

#[derive(Debug, PartialEq, Eq)]
struct ActiveNote {
    path: PathBuf,
    text: String,
    revision: u64,
    cursor: usize,
    anchor: usize,
    dirty: bool,
    tabs: usize,
    workspace_revision: u64,
}

fn active_note(view: &gpui::Entity<Workspace>, cx: &gpui::VisualTestContext) -> ActiveNote {
    view.read_with(cx, |this, cx| {
        let tab = &this.tabs[this.active];
        let editor = tab.editor.read(cx);
        ActiveNote {
            path: tab.path.clone(),
            text: editor.buffer.text().to_owned(),
            revision: editor.buffer.revision(),
            cursor: editor.buffer.cursor(),
            anchor: editor.buffer.anchor(),
            dirty: tab.dirty(cx),
            tabs: this.tabs.len(),
            workspace_revision: this.revision,
        }
    })
}

fn click_action(view: &gpui::Entity<Workspace>, cx: &mut gpui::VisualTestContext, label: &str) {
    let index = view.read_with(cx, |this, _| {
        this.explorer_actions()
            .iter()
            .position(|(_, text, _)| *text == label)
            .unwrap_or_else(|| panic!("missing menu command {label}"))
    });
    // The native harness requires static selector strings.
    let selectors = [
        "explorer-action-0",
        "explorer-action-1",
        "explorer-action-2",
        "explorer-action-3",
        "explorer-action-4",
        "explorer-action-5",
        "explorer-action-6",
        "explorer-action-7",
    ];
    let position = cx.debug_bounds(selectors[index]).unwrap().center();
    cx.simulate_click(position, gpui::Modifiers::default());
    // GPUI Frame::clear retains debug_bounds for removed selectors. The live
    // menu state drives rendering; clipboard assertions below verify the click.
    view.read_with(cx, |this, _| assert!(this.explorer_menu.is_none()));
}

fn assert_clipboard(view: &gpui::Entity<Workspace>, cx: &gpui::VisualTestContext, expected: &str) {
    view.read_with(cx, |this, cx| {
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some(expected)
        );
        assert!(this.modal == Modal::None);
        assert!(this.inline_parent.is_none());
        assert!(!this.managing);
        assert!(this.explorer_menu.is_none());
    });
}

#[gpui::test]
fn clicked_note_and_folder_copy_literal_paths_without_touching_active_dirty_note(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    let target = PathBuf::from("Ideas é/Deep space/Draft's [v1] #1.md");
    std::fs::create_dir_all(folder.path().join("Ideas é/Deep space")).unwrap();
    std::fs::write(folder.path().join(&target), "target note\n").unwrap();
    std::fs::write(folder.path().join("B.md"), "active note\n").unwrap();
    let absolute_root = folder.path().canonicalize().unwrap();
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
    click(cx, "entry:Ideas é");
    click(cx, "entry:Ideas é/Deep space");
    view.update_in(cx, |this, window, cx| {
        this.open_note("B.md".into(), window, cx);
        this.set_mode(ViewMode::Source, window, cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-end");
    cx.simulate_input("unsaved edits");
    let before = active_note(&view, cx);
    assert_eq!(before.path, PathBuf::from("B.md"));
    assert!(before.dirty);
    for (relative, selector) in [
        (
            target.clone(),
            "entry:Ideas é/Deep space/Draft's [v1] #1.md",
        ),
        (
            PathBuf::from("Ideas é/Deep space"),
            "entry:Ideas é/Deep space",
        ),
    ] {
        right_click(cx, selector);
        view.read_with(cx, |this, _| {
            assert_eq!(
                this.explorer_menu.as_ref().unwrap().target.as_ref(),
                Some(&relative)
            )
        });
        click_action(&view, cx, "Copy path");
        assert_clipboard(&view, cx, &absolute_root.join(&relative).to_string_lossy());
        assert_eq!(active_note(&view, cx), before);

        right_click(cx, selector);
        click_action(&view, cx, "Copy relative path");
        assert_clipboard(&view, cx, &relative.to_string_lossy());
        assert_eq!(active_note(&view, cx), before);
    }
    assert_eq!(
        std::fs::read_to_string(folder.path().join(&target)).unwrap(),
        "target note\n"
    );
    assert_eq!(
        std::fs::read_to_string(folder.path().join("B.md")).unwrap(),
        "active note\n"
    );
    assert!(folder.path().join("Ideas é/Deep space").is_dir());
}

#[gpui::test]
fn keyboard_context_menu_copies_clicked_note_and_folder_paths(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("Ébauches space")).unwrap();
    std::fs::write(folder.path().join("A space.md"), "A body").unwrap();
    std::fs::write(folder.path().join("B.md"), "B body").unwrap();
    let absolute_root = folder.path().canonicalize().unwrap();
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
    view.update_in(cx, |this, window, cx| {
        this.open_note("B.md".into(), window, cx)
    });
    cx.run_until_parked();
    let before = active_note(&view, cx);
    for (relative, selector) in [
        (PathBuf::from("A space.md"), "entry:A space.md"),
        (PathBuf::from("Ébauches space"), "entry:Ébauches space"),
    ] {
        // Opening the existing menu selects its clicked target and focuses the
        // tree. Shift+F10 must reuse that selection while B remains active.
        right_click(cx, selector);
        cx.simulate_keystrokes("escape shift-f10 up enter");
        assert_clipboard(&view, cx, &relative.to_string_lossy());
        assert_eq!(active_note(&view, cx), before);

        cx.simulate_keystrokes("shift-f10 up up space");
        assert_clipboard(&view, cx, &absolute_root.join(&relative).to_string_lossy());
        assert_eq!(active_note(&view, cx), before);
    }
    assert_eq!(
        std::fs::read_to_string(folder.path().join("A space.md")).unwrap(),
        "A body"
    );
    assert_eq!(
        std::fs::read_to_string(folder.path().join("B.md")).unwrap(),
        "B body"
    );
}

#[gpui::test]
fn root_menu_copies_absolute_root_and_dot_without_opening_a_dialog(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("B.md"), "unchanged").unwrap();
    let absolute_root = folder.path().canonicalize().unwrap();
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
    let before = active_note(&view, cx);
    right_click_explorer_space(cx);
    click_action(&view, cx, "Copy path");
    assert_clipboard(&view, cx, &absolute_root.to_string_lossy());
    assert_eq!(active_note(&view, cx), before);
    right_click_explorer_space(cx);
    click_action(&view, cx, "Copy relative path");
    assert_clipboard(&view, cx, ".");
    assert_eq!(active_note(&view, cx), before);
    cx.simulate_keystrokes("shift-f10 up up enter");
    assert_clipboard(&view, cx, &absolute_root.to_string_lossy());
    assert_eq!(active_note(&view, cx), before);
    assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 1);
    assert_eq!(
        std::fs::read_to_string(folder.path().join("B.md")).unwrap(),
        "unchanged"
    );
}
