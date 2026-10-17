//! Workspace startup, child entities and background vault observation.
use super::*;

impl Workspace {
    pub fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state_directory = if cfg!(test) {
            None
        } else {
            navigation::state_directory()
        };
        let preferences = state_directory
            .as_deref()
            .map(opalglass::state::Preferences::load)
            .unwrap_or_default();
        let search = cx.new(|cx| Editor::new("", true, "Search notes…", cx));
        let input = cx.new(|cx| Editor::new("", true, "", cx));
        let find_input = cx.new(|cx| Editor::new("", true, "Find in note…", cx));
        let owner = cx.entity().downgrade();
        let sidebar = cx.new(|_| Sidebar {
            #[cfg(test)]
            render_count: 0,
            owner: owner.clone(),
        });
        let preview_pane = cx.new(|_| PreviewPane {
            pending_sync: None,
            #[cfg(test)]
            render_count: 0,
            owner: owner.clone(),
        });
        let remote_images = cx.new(|cx| remote_images::Cache::new(preview_pane.downgrade(), cx));
        let embedded_documents = cx
            .new(|cx| embedded_documents::Cache::new(owner.clone(), preview_pane.downgrade(), cx));
        let connections = cx.new(|_| Connections {
            #[cfg(test)]
            render_count: 0,
            owner,
            items: Arc::new(Vec::new()),
            backlinks: 0,
            tags: 0,
            empty_message: "No incoming links yet",
            scroll: UniformListScrollHandle::new(),
        });
        let subscriptions = vec![
            cx.observe(&search, |_, _, cx| cx.notify()),
            cx.observe(&input, |_, _, cx| cx.notify()),
            cx.observe(&find_input, |_, _, cx| cx.notify()),
            cx.observe(&cx.entity(), |this, _, cx| {
                // Invalidate cached children before GPUI starts the next frame.
                this.update_indexing(cx);
                this.update_link_styles(cx);
                this.sync_auto_reveal();
                this.update_panes(cx);
                this.invalidate_embed_sources(cx);
                this.sidebar.update(cx, |_, cx| cx.notify());
            }),
        ];
        let watch = cx.spawn(async |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(3)).await;
                let Ok(scan) = this.update(cx, |this, cx| {
                    this.persist_navigation(cx);
                    this.checkpoint(cx);
                    (!this.managing && !this.loading && !this.indexing && !this.checkpointing)
                        .then(|| this.vault.as_ref().map(|v| (v.clone(), this.revision)))
                        .flatten()
                }) else {
                    break;
                };
                let Some((mut vault, revision)) = scan else {
                    continue;
                };
                let result = cx
                    .background_executor()
                    .spawn(async move { vault.refresh().map(|changed| changed.then_some(vault)) })
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if this.revision != revision {
                            return;
                        }
                        match result {
                            Ok(Some(vault)) => this.apply_scan(vault, revision, cx),
                            Ok(None) => (),
                            Err(error) => {
                                this.status = format!("Could not refresh vault: {error}");
                                this.error = true;
                                cx.notify();
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut workspace = Self {
            vault: None,
            tabs: Vec::new(),
            history_back: Vec::new(),
            history_forward: Vec::new(),
            history_restore: None,
            history_request: false,
            active: 0,
            focus: cx.focus_handle(),
            search,
            completion: LinkCompletion::default(),
            find: NoteFind {
                input: find_input,
                open: false,
                case_sensitive: false,
                key: None,
                matches: Arc::new(Vec::new()),
                selected: None,
                generation: 0,
                task: None,
                searching: false,
            },
            input,
            modal: Modal::None,
            inline_parent: None,
            explorer_menu: None,
            sort_menu: None,
            sort_selected: 0,
            auto_revealed: None,
            move_candidates: Vec::new(),
            move_results: Vec::new(),
            move_query: String::new(),
            move_selected: 0,
            move_scroll: UniformListScrollHandle::new(),
            tab_drop: None,
            tabs_scroll: ScrollHandle::new(),
            mode: ViewMode::Split,
            preferences: preferences.clone(),
            settings_open: false,
            properties_panel: None,
            history_panel: None,
            _history: None,
            comparison: None,
            _comparison: None,
            mentions: None,
            _mentions: None,
            replace_panel: None,
            _replace: None,
            picker: None,
            picker_query: None,
            picker_selected: 0,
            picker_items: Vec::new(),
            picker_scroll: UniformListScrollHandle::new(),
            outline_open: false,
            outline_scroll: UniformListScrollHandle::new(),
            sidebar,
            preview_pane,
            remote_images,
            embedded_documents,
            preview_revision: None,
            connections,
            connections_revision: None,
            #[cfg(debug_assertions)]
            performance: cx.new(|_| PerformanceBar::default()),
            indexing_bar: cx.new(|_| IndexingBar::default()),
            _index_progress: None,
            status: "Your notes, on your disk.".into(),
            error: false,
            revision: 0,
            load_generation: 0,
            loading: false,
            _load: None,
            load_cancelled: Arc::new(AtomicBool::new(false)),
            indexing: false,
            index_revision: u64::MAX,
            index_cancelled: Arc::new(AtomicBool::new(false)),
            _index: None,
            search_query: String::new(),
            search_revision: u64::MAX,
            search_results: Vec::new(),
            search_index: None,
            search_engine: None,
            search_error: None,
            search_selected: 0,
            snippet_cache: Arc::new(Mutex::new(Snippets::default())),
            _snippet_task: None,
            snippets_loading: false,
            snippet_requests: Vec::new(),
            searching: false,
            open_search_result: false,
            _search_task: None,
            search_cancelled: Arc::new(AtomicBool::new(false)),
            notes_scroll: UniformListScrollHandle::new(),
            results_scroll: UniformListScrollHandle::new(),
            tree: Tree::default(),
            navigation: NavigationState::default(),
            tree_revision: u64::MAX,
            tree_focus: cx.focus_handle(),
            resizing_sidebar: false,
            split_ratio: 0.5,
            scroll_locked: false,
            live_preview: preferences.default_live_preview,
            split_bounds: Rc::new(Cell::new(Bounds::default())),
            split_drag_offset: None,
            typeahead: String::new(),
            typeahead_at: std::time::Instant::now(),
            state_directory,
            persisted_navigation: None,
            modal_target: None,
            managing: false,
            _management: None,
            note_generation: 0,
            _note_load: None,
            note_loading: false,
            _restore: None,
            checkpointing: false,
            checkpoints: Default::default(),
            _checkpoint: None,
            restoring: false,
            _subscriptions: subscriptions,
            _watch: watch,
        };
        workspace.focus.focus(window);
        let path = path.or_else(|| {
            workspace
                .state_directory
                .as_ref()
                .and_then(|dir| opalglass::state::last_vault(dir))
        });
        if let Some(path) = path {
            workspace.open_vault(path, window, cx);
        }
        workspace
    }
}
