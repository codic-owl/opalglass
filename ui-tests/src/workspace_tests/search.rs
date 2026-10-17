use super::*;

#[gpui::test]
fn tag_filter_matches_metadata_and_nested_tags_without_plain_word_hits(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    for (path, text) in [
        ("A.md", "#work and #other"),
        ("B.md", "---\ntags: [work/research]\n---\nBody"),
        ("C.md", "work #workshop `#work` [[work|#work]]"),
    ] {
        std::fs::write(folder.path().join(path), text).unwrap();
    }
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    view.update_in(cx, |this, window, cx| {
        this.navigation.hidden = true;
        this.follow(
            format!("{}work", opalglass::markdown::TAG_TARGET_PREFIX),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    // Debounced search runs on the test executor's clock.
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(20));
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert!(!this.navigation.hidden);
        assert_eq!(this.search.read(cx).buffer.text(), "tag:work");
        assert_eq!(
            this.search_results
                .iter()
                .map(|r| r.path.clone())
                .collect::<Vec<_>>(),
            vec![PathBuf::from("A.md"), PathBuf::from("B.md")]
        );
    });
}

#[gpui::test]
fn discovered_vault_is_editable_and_pending_search_completes_after_indexing(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "[[B]] actual content").unwrap();
    std::fs::write(folder.path().join("B.md"), "needle-only-in-body").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(None, w, cx));
    view.update_in(cx, |this, w, cx| {
        let vault = opalglass::vault::Vault::discover(
            folder.path(),
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
        this.finish_open(Ok((vault, Default::default())), w, cx);
        // Hold the background phase so the test can interact with the
        // independently usable path inventory before indexing finishes.
        this.index_revision = this.revision;
    });
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tree.visible.len(), 2);
        assert!(!this.vault.as_ref().unwrap().content_indexed());
        assert!(
            this.vault
                .as_ref()
                .unwrap()
                .notes
                .iter()
                .all(|n| n.matches_text(""))
        );
        assert_eq!(
            this.tabs[this.active].editor.read(cx).buffer.text(),
            "[[B]] actual content"
        );
        assert!(!this.link_highlight("B").underline.unwrap().wavy);
    });
    cx.simulate_keystrokes("ctrl-end");
    cx.simulate_input(" edited");
    cx.simulate_keystrokes("ctrl-p");
    cx.simulate_input("needle-only-in-body");
    cx.simulate_keystrokes("enter");
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(150));
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert!(this.search_results.is_empty());
        assert!(this.open_search_result);
    });
    view.update(cx, |this, cx| {
        this.index_revision = u64::MAX;
        cx.notify();
    });
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(150));
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        assert!(this.vault.as_ref().unwrap().content_indexed());
        assert_eq!(this.tabs[this.active].path, PathBuf::from("B.md"));
        let a = this
            .tabs
            .iter()
            .find(|t| t.path == PathBuf::from("A.md"))
            .unwrap();
        assert_eq!(
            a.editor.read(cx).buffer.text(),
            "[[B]] actual content edited"
        );
        assert!(a.dirty(cx));
        assert!(!this.error, "{}", this.status);
    });
}

#[gpui::test]
fn indexing_restarts_after_save_and_is_cancelled_on_vault_switch(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "old").unwrap();
    std::fs::write(other.path().join("Other.md"), "other").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    let cancelled = view.update(cx, |this, cx| {
        this.vault = Some(
            opalglass::vault::Vault::discover(
                folder.path(),
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap(),
        );
        this.revision += 1;
        this.update_indexing(cx);
        let cancelled = this.index_cancelled.clone();
        this.tabs[0].editor.update(cx, |editor, cx| {
            editor.set_text("saved during indexing", cx)
        });
        assert!(this.save_tab(0, cx));
        cancelled
    });
    cx.run_until_parked();
    assert!(cancelled.load(std::sync::atomic::Ordering::Relaxed));
    view.read_with(cx, |this, cx| {
        assert!(this.vault.as_ref().unwrap().content_indexed());
        assert!(this.vault.as_ref().unwrap().notes[0].matches_text("saved during indexing"));
        assert_eq!(
            this.tabs[0].editor.read(cx).buffer.text(),
            "saved during indexing"
        );
        assert!(!this.tabs[0].dirty(cx));
    });
    let cancelled = view.update_in(cx, |this, w, cx| {
        this.vault = Some(
            opalglass::vault::Vault::discover(
                folder.path(),
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap(),
        );
        this.revision += 1;
        this.update_indexing(cx);
        let cancelled = this.index_cancelled.clone();
        this.open_vault(other.path().into(), w, cx);
        cancelled
    });
    cx.run_until_parked();
    assert!(cancelled.load(std::sync::atomic::Ordering::Relaxed));
    view.read_with(cx, |this, _| {
        assert_eq!(
            this.vault.as_ref().unwrap().root,
            other.path().canonicalize().unwrap()
        );
        assert_eq!(this.tabs[0].path, PathBuf::from("Other.md"));
        assert!(this.vault.as_ref().unwrap().content_indexed());
    });
}

#[gpui::test]
fn indexing_footer_tracks_progress_without_redrawing_notes_and_hides_on_cancel(
    cx: &mut TestAppContext,
) {
    use super::VaultIndexProgress;
    use opalglass::search::SyncProgress;
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "# Note\nBody").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    assert!(cx.debug_bounds("indexing-vault").is_none());
    view.update(cx, |this, cx| {
        this.indexing = true;
        this.indexing_bar.update(cx, |bar, cx| {
            bar.progress = VaultIndexProgress::Reading {
                completed: 0,
                total: 10,
            };
            cx.notify();
        });
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(
        cx.debug_bounds("indexing-vault").unwrap().size.width,
        gpui::px(180.)
    );
    let renders = view.read_with(cx, |this, cx| this.tabs[0].editor.read(cx).render_count);
    view.update(cx, |this, cx| {
        this.indexing_bar.update(cx, |bar, cx| {
            bar.progress = VaultIndexProgress::Search(SyncProgress::Notes {
                completed: 5,
                total: 10,
            });
            cx.notify();
        })
    });
    cx.run_until_parked();
    assert_eq!(
        cx.debug_bounds("indexing-progress-fill")
            .unwrap()
            .size
            .width,
        gpui::px(32.)
    );
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[0].editor.read(cx).render_count, renders)
    });
    view.update(cx, |this, cx| {
        this.indexing_bar.update(cx, |bar, cx| {
            bar.progress = VaultIndexProgress::Search(SyncProgress::Finalizing);
            assert!(bar.progress.fraction() < 1.);
            assert_eq!(bar.progress.detail(), "Finishing search index…");
            cx.notify();
        })
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("indexing-vault").is_some());
    view.update(cx, |this, cx| {
        this.cancel_indexing();
        this.index_revision = this.revision;
        cx.notify();
    });
    cx.run_until_parked();
    // GPUI retains removed debug selectors in its frame map; inspect
    // visibility state here and verify the disappearing footer natively.
    view.read_with(cx, |this, _| {
        assert!(!this.indexing);
        assert!(this._index_progress.is_none());
    });
    assert!(
        VaultIndexProgress::Reading {
            completed: 0,
            total: 0
        }
        .fraction()
        .is_finite()
    );
}

#[gpui::test]
fn search_starts_after_first_paint_with_preloaded_vault(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "already loaded contents").unwrap();
    let vault = opalglass::vault::Vault::open(folder.path()).unwrap();
    assert!(vault.content_indexed());
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(None, w, cx));
    view.update_in(cx, |this, w, cx| {
        this.finish_open(Ok((vault, Default::default())), w, cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert!(this.search_ready(), "{}", this.status)
    });
}

#[gpui::test]
fn indexed_search_ranks_typos_highlights_and_loads_only_visible_excerpts(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(
        folder.path().join("Glossary.md"),
        "Glossary definitions and quartz.",
    )
    .unwrap();
    std::fs::write(folder.path().join("Repeated.md"), "foo foo foo").unwrap();
    for i in 0..60 {
        std::fs::write(
            folder.path().join(format!("Note {i:02}.md")),
            format!("quartz sample number {i}."),
        )
        .unwrap();
    }
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-p");
    cx.simulate_input("glossry");
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(50));
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert!(this.search_ready(), "{}", this.status);
        assert_eq!(this.search_results[0].title, "Glossary");
        let snippet = this.search_results[0].snippet.as_ref().unwrap();
        assert!(
            snippet
                .highlights
                .iter()
                .any(|r| &snippet.fragment[r.clone()] == "Glossary")
        );
    });
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("quartz");
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(50));
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert_eq!(this.search_results.len(), 61);
        let loaded = this
            .search_results
            .iter()
            .filter(|r| r.snippet.is_some())
            .count();
        assert!(loaded > 0 && loaded < 20, "{loaded} snippets for viewport");
    });
    view.update(cx, |this, cx| {
        this.results_scroll
            .scroll_to_item(60, gpui::ScrollStrategy::Center);
        this.sidebar.update(cx, |_, cx| cx.notify());
    });
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert!(this.search_results[60].snippet.is_some())
    });
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert_eq!(
            this.tabs[this.active].path,
            this.search_results[this.search_selected].path
        )
    });
    cx.simulate_keystrokes("ctrl-p ctrl-a");
    cx.simulate_input("\"foo foo\"");
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(50));
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert_eq!(
            this.search_results[0].snippet.as_ref().unwrap().highlights,
            vec![0..11]
        )
    });
}

#[gpui::test]
fn indexed_search_tracks_saved_external_renamed_and_removed_notes(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "alpha").unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    cx.run_until_parked();
    view.update(cx, |this, cx| {
        this.tabs[0]
            .editor
            .update(cx, |e, cx| e.set_text("nebula", cx));
        assert!(this.save_tab(0, cx));
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-p");
    cx.simulate_input("nebula");
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(50));
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert_eq!(this.search_results[0].path, PathBuf::from("A.md"))
    });
    std::fs::rename(folder.path().join("A.md"), folder.path().join("Moved.md")).unwrap();
    std::fs::write(folder.path().join("Moved.md"), "supernova").unwrap();
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(3));
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-p ctrl-a");
    cx.simulate_input("supernova");
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(50));
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert!(
            !this.search_results.is_empty(),
            "status={} ready={} rev={} index={} searching={} query={} paths={:?}",
            this.status,
            this.search_ready(),
            this.revision,
            this.index_revision,
            this.searching,
            this.search_query,
            this.vault
                .as_ref()
                .unwrap()
                .notes
                .iter()
                .map(|n| &n.path)
                .collect::<Vec<_>>()
        );
        assert_eq!(this.search_results[0].path, PathBuf::from("Moved.md"));
    });
    std::fs::remove_file(folder.path().join("Moved.md")).unwrap();
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(3));
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(50));
    cx.run_until_parked();
    view.read_with(cx, |this, _| assert!(this.search_results.is_empty()));
}

#[gpui::test]
fn background_search_discards_old_queries_and_enter_waits_for_results(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("A.md"), "alpha").unwrap();
    std::fs::write(folder.path().join("B.md"), "beta").unwrap();
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
    cx.simulate_keystrokes("ctrl-p");
    cx.simulate_input("alpha");
    let cancelled = view.read_with(cx, |this, _| this.search_cancelled.clone());
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("beta");
    cx.simulate_keystrokes("enter");
    view.read_with(cx, |this, _| {
        assert!(this.searching);
        assert!(this.open_search_result);
        assert!(this.search_results.is_empty());
    });
    assert!(cancelled.load(std::sync::atomic::Ordering::Relaxed));
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(150));
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert!(!this.searching);
        assert_eq!(this.search_results.len(), 1);
        assert_eq!(this.tabs[this.active].path, PathBuf::from("B.md"));
    });
}
