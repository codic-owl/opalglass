// Vault discovery, content indexing and cancellation.
use super::*;

impl Workspace {
    pub(super) fn apply_scan(&mut self, vault: Vault, revision: u64, cx: &mut Context<Self>) {
        if self.managing
            || self.checkpointing
            || self.revision != revision
            || self.vault.as_ref().is_none_or(|v| v.root != vault.root)
        {
            return;
        }
        let mut reload = Vec::new();
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            match vault.notes.iter().find(|n| n.path == tab.path) {
                Some(note) if !note.matches_text(&tab.baseline) => {
                    tab.external_changed = true;
                    if !tab.dirty(cx) {
                        reload.push(index);
                    } else {
                        self.status = "External changes detected. Compare versions to review your edits and the disk version.".into();
                        self.error = true;
                    }
                }
                None => {
                    tab.external_changed = true;
                    self.status =
                        "An open note was removed from disk. Save a copy to preserve its contents."
                            .into();
                    self.error = true;
                }
                _ => tab.external_changed = false,
            }
        }
        self.vault = Some(vault);
        self.revision += 1;
        for index in reload {
            self.reload_tab(index, cx);
        }
        cx.notify();
    }
    pub(super) fn open_vault(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let started = Instant::now();
        let target = path.display().to_string();
        if !self.save_all_in(window, cx) {
            self.log_performance("Vault switch blocked", target, started, cx);
            return;
        }
        self.cancel_indexing();
        self.load_cancelled.store(true, Ordering::Relaxed);
        self.load_cancelled = Arc::new(AtomicBool::new(false));
        let cancelled = self.load_cancelled.clone();
        self.load_generation += 1;
        self.note_generation += 1;
        self._note_load = None;
        self.note_loading = false;
        let generation = self.load_generation;
        self.loading = true;
        self.modal = Modal::None;
        self.inline_parent = None;
        self.explorer_menu = None;
        self.sort_menu = None;
        self.focus_document(window, cx);
        self.message("Opening vault…", false, cx);
        let state_directory = self.state_directory.clone();
        self._load = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    Vault::discover(path, &cancelled).map(|vault| {
                        let state = state_directory
                            .map(|d| NavigationState::load(&d, &vault.root))
                            .unwrap_or_default();
                        (vault, state)
                    })
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.load_generation != generation {
                    return;
                }
                this.loading = false;
                // The previous vault remains usable while loading. Preserve any
                // edits made since the request, and abort switching on conflict.
                if !this.save_all_in(window, cx) {
                    this.log_performance("Vault switch blocked", target, started, cx);
                    return;
                }
                let action = if result.is_ok() {
                    "Open vault"
                } else {
                    "Vault open failed"
                };
                this.finish_open(result, window, cx);
                this.log_performance(action, target, started, cx);
            });
        }));
    }
    pub(super) fn finish_open(
        &mut self,
        result: std::io::Result<(Vault, NavigationState)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok((vault, state)) => {
                self.note_generation += 1;
                self._note_load = None;
                self.note_loading = false;
                self.revision += 1;
                let first = vault
                    .notes
                    .iter()
                    .find(|n| n.title == "Welcome")
                    .or_else(|| vault.notes.first())
                    .map(|n| n.path.clone());
                self._restore = None;
                self.restoring = false;
                self.navigation = state;
                self.auto_revealed = None;
                self.persisted_navigation = Some(self.navigation.clone());
                self.tree
                    .rebuild(&vault, &self.navigation.expanded, self.navigation.sort);
                self.tree_revision = self.revision;
                self.vault = Some(vault);
                if let Some(index) = self.search_index.take() {
                    cx.background_executor()
                        .spawn(async move {
                            drop(index);
                        })
                        .detach();
                }
                self.search_engine = None;
                self.search_error = None;
                self.search_results.clear();
                self.search_revision = u64::MAX;
                self.snippet_cache = Arc::new(Mutex::new(Snippets::default()));
                self.tabs.clear();
                self.history_back.clear();
                self.history_forward.clear();
                self.history_restore = None;
                self.active = 0;
                self.notes_scroll = UniformListScrollHandle::new();
                self.notes_scroll
                    .0
                    .borrow()
                    .base_handle
                    .set_offset(point(px(0.), px(self.navigation.scroll_y)));
                self.results_scroll = UniformListScrollHandle::new();
                self.search.update(cx, |editor, cx| editor.set_text("", cx));
                self.modal = Modal::None;
                self.inline_parent = None;
                self.explorer_menu = None;
                self.sort_menu = None;
                self.message("Vault opened", false, cx);
                if let (Some(dir), Some(vault)) = (&self.state_directory, &self.vault)
                    && let Err(error) =
                        opalglass::state::write_json(&dir.join("last-vault.json"), &vault.root)
                {
                    self.message(format!("Could not remember vault: {error}"), true, cx);
                }
                if self.navigation.session_initialized {
                    self.restore_session(window, cx);
                } else if let Some(path) = first {
                    self.open_note(path, window, cx);
                } else {
                    self.focus.focus(window);
                }
            }
            Err(error) => self.message(format!("Could not open vault: {error}"), true, cx),
        }
    }
    pub(super) fn cancel_indexing(&mut self) {
        self.index_cancelled.store(true, Ordering::Relaxed);
        self._index = None;
        self._index_progress = None;
        self.indexing = false;
        self.index_revision = u64::MAX;
    }
    pub(super) fn update_indexing(&mut self, cx: &mut Context<Self>) {
        // Let the first note open before scheduling the bulk index, including
        // on machines with only one background worker.
        if self.loading
            || self.managing
            || self.checkpointing
            || (self.note_loading && self.tabs.is_empty())
            || self
                .tabs
                .get(self.active)
                .is_some_and(|tab| tab.initial_frame_pending)
        {
            return;
        }
        let Some(vault) = &self.vault else { return };
        if self.index_revision == self.revision {
            return;
        }
        let mut vault = vault.clone();
        self.cancel_indexing();
        self.index_cancelled = Arc::new(AtomicBool::new(false));
        let cancelled = self.index_cancelled.clone();
        let revision = self.revision;
        let generation = self.load_generation;
        let existing = self.search_index.clone();
        let cache = if cfg!(test) {
            None
        } else {
            opalglass::search::cache_directory()
        };
        self.index_revision = revision;
        self.indexing = true;
        self.search_error = None;
        let total = vault.notes.len();
        let initial = if vault.content_indexed() {
            VaultIndexProgress::Search(opalglass::search::SyncProgress::Notes {
                completed: 0,
                total,
            })
        } else {
            VaultIndexProgress::Reading {
                completed: 0,
                total,
            }
        };
        let progress = Arc::new(Mutex::new(initial));
        self.indexing_bar.update(cx, |bar, cx| {
            bar.progress = initial;
            cx.notify();
        });
        let observed = progress.clone();
        let bar = self.indexing_bar.downgrade();
        // Update only the small footer entity, at most five times a second.
        // A distinct progress handle per job prevents stale/cancelled updates.
        self._index_progress = Some(cx.spawn(async move |_, cx| {
            loop {
                Timer::after(Duration::from_millis(200)).await;
                let latest = *observed.lock().unwrap();
                if bar
                    .update(cx, |bar, cx| {
                        if bar.progress != latest {
                            bar.progress = latest;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        self._index = Some(cx.spawn(async move |this, cx| {
            let task_cancelled = cancelled.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let changed = if !vault.content_indexed() {
                        vault.refresh_with_progress(&cancelled, |completed| {
                            *progress.lock().unwrap() =
                                VaultIndexProgress::Reading { completed, total };
                        })?
                    } else {
                        false
                    };
                    let index = match existing {
                        Some(index) => index,
                        None => Arc::new(
                            SearchIndex::open(&vault.root, cache.as_deref())
                                .map_err(std::io::Error::other)?,
                        ),
                    };
                    let paths = vault
                        .notes
                        .iter()
                        .map(|n| n.path.clone())
                        .collect::<Vec<_>>();
                    index
                        .synchronize_with_progress(&paths, &cancelled, |state| {
                            *progress.lock().unwrap() = VaultIndexProgress::Search(state);
                        })
                        .map_err(std::io::Error::other)?;
                    let engine = Arc::new(index.snapshot().map_err(std::io::Error::other)?);
                    Ok::<_, std::io::Error>((vault, changed, index, engine))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if task_cancelled.load(Ordering::Relaxed)
                    || this.revision != revision
                    || this.load_generation != generation
                {
                    return;
                }
                this.indexing = false;
                this._index_progress = None;
                match result {
                    Ok((vault, changed, index, engine)) => {
                        if changed {
                            this.apply_scan(vault, revision, cx);
                        }
                        this.search_index = Some(index);
                        this.search_engine = Some(engine);
                        this.index_revision = this.revision;
                        this.search_revision = u64::MAX;
                    }
                    Err(error) => {
                        this.search_error = Some(error.to_string());
                        this.search_revision = u64::MAX;
                        this.message(format!("Could not update search: {error}"), true, cx);
                    }
                }
                cx.notify();
            });
        }));
    }
    pub(super) fn search_ready(&self) -> bool {
        self.search_engine.is_some()
            && !self.indexing
            && self.index_revision == self.revision
            && self.search_error.is_none()
    }
    pub(super) fn pick_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Markdown vault".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = result.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        this.open_vault(path, window, cx);
                    }
                }
                Ok(Ok(None)) => (),
                _ => {
                    this.show_modal(Modal::OpenPath, window, cx);
                    this.message(
                        "The folder picker is unavailable. Enter a vault path below.",
                        true,
                        cx,
                    );
                }
            });
        })
        .detach();
    }
}
