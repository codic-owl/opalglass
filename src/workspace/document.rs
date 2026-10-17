// Document preparation, link styles, reload and pane synchronization.
use super::*;

impl Workspace {
    pub(super) fn sync_reading_scroll(&mut self, cx: &mut Context<Self>) {
        if !self.scroll_locked || self.mode != ViewMode::Split {
            return;
        }
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let Some((revision, byte)) = tab.editor.read(cx).scroll_anchor else {
            return;
        };
        if revision != tab.prepared_revision || tab.prepared.rows.is_empty() {
            return;
        }
        let index = tab.prepared.row_at_source(byte as usize);
        let range = &tab.prepared.rows[index].source;
        let fraction =
            ((byte - range.start as f64) / range.len().max(1) as f64).clamp(0., 1.) as f32;
        let host_index = tab.preview_projection.list_index(index);
        let fraction = if tab.preview_projection.row(host_index) == index {
            fraction
        } else {
            0.
        };
        let index = tab.composition.projection.list_index(host_index);
        let height = tab
            .preview_scroll
            .bounds_for_item(index)
            .map_or(px(0.), |bounds| bounds.size.height);
        tab.preview_scroll.scroll_to(ListOffset {
            item_ix: index,
            offset_in_item: height * fraction,
        });
        let scroll = tab.preview_scroll.clone();
        self.preview_pane.update(cx, |pane, cx| {
            // An offscreen row has no measured height yet. Correct its fractional
            // offset after it is laid out, without measuring the rest of the note.
            pane.pending_sync = Some((scroll, index, fraction));
            cx.notify();
        });
    }
    pub(super) fn update_panes(&mut self, cx: &mut Context<Self>) {
        self.sync_reading_scroll(cx);
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let key = (
            tab.editor.entity_id(),
            tab.prepared_revision,
            self.revision,
            tab.prepared.complete,
        );
        if self.preview_revision != Some(key) {
            self.preview_revision = Some(key);
            self.preview_pane.update(cx, |_, cx| cx.notify());
        }
        let key = (key.0, key.1, key.2, key.3, self.indexing);
        if self.connections_revision == Some(key) {
            return;
        }
        self.connections_revision = Some(key);
        let revision = (key.1, key.2, key.3, key.4);
        let snapshot = match &tab.connections_cache {
            Some(cached) if cached.revision == revision => cached.clone(),
            _ => {
                let backlinks = self
                    .vault
                    .as_ref()
                    .map(|v| v.backlinks(&tab.path))
                    .unwrap_or_default();
                let count = backlinks.len();
                let mut items: Vec<_> = backlinks
                    .into_iter()
                    .map(|note| Connection::Note(note.path.clone(), note.title.clone()))
                    .collect();
                items.extend(tab.prepared.tags.iter().cloned().map(Connection::Tag));
                let message = if self.vault.as_ref().is_some_and(|v| !v.content_indexed()) {
                    if self.indexing {
                        "Indexing backlinks…"
                    } else {
                        "Backlinks not ready"
                    }
                } else {
                    "No incoming links yet"
                };
                let snapshot = ConnectionSnapshot {
                    revision,
                    items: Arc::new(items),
                    backlinks: count,
                    tags: tab.prepared.tags.len(),
                    empty_message: message,
                };
                tab.connections_cache = Some(snapshot.clone());
                snapshot
            }
        };
        let scroll = tab.connections_scroll.clone();
        self.connections.update(cx, |panel, cx| {
            panel.items = snapshot.items;
            panel.backlinks = snapshot.backlinks;
            panel.tags = snapshot.tags;
            panel.empty_message = snapshot.empty_message;
            panel.scroll = scroll;
            cx.notify();
        });
    }
    pub(super) fn update_document(&mut self, cx: &mut Context<Self>) {
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            if index != self.active {
                tab.cancel_opening_analysis();
            }
        }
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let revision = tab.editor.read(cx).buffer.revision();
        if tab.initial_frame_pending || tab.requested_revision == revision {
            return;
        }
        let initial = !tab.prepared.analysis_complete();
        let decorate = tab.prepared.complete
            && tab.prepared_revision == revision
            && !tab.prepared.syntax_ready;
        tab.requested_revision = revision;
        tab.prepare_cancelled.store(true, Ordering::Relaxed);
        tab.prepare_cancelled = Arc::new(AtomicBool::new(false));
        let cancelled = tab.prepare_cancelled.clone();
        let text = tab.editor.read(cx).buffer.snapshot();
        let editor_id = tab.editor.entity_id();
        let live_context = decorate.then(|| tab.prepared.live.clone());
        tab._prepare = Some(cx.spawn(async move |this, cx| {
            let task_cancelled = cancelled.clone();
            if !initial {
                cx.background_executor()
                    .timer(Duration::from_millis(75))
                    .await;
            }
            let prepared = cx
                .background_executor()
                .spawn(async move {
                    if decorate {
                        Prepared::new_cancellable(&text, &cancelled)
                    } else {
                        Prepared::undecorated_cancellable(&text, &cancelled)
                    }
                    .map(|mut prepared| {
                        if let Some(context) = live_context {
                            prepared.live = context;
                        }
                        Arc::new(prepared)
                    })
                })
                .await;
            let Some(prepared) = prepared else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                let Some(tab) = this
                    .tabs
                    .iter_mut()
                    .find(|t| t.editor.entity_id() == editor_id)
                else {
                    return;
                };
                if task_cancelled.load(Ordering::Relaxed)
                    || tab.editor.read(cx).buffer.revision() != revision
                {
                    return;
                }
                tab.install_prepared(prepared.clone(), tab.prepared_revision == revision);
                if let Some(pending) = tab.pending_preview.take() {
                    tab.restore_preview_offset(pending);
                }
                let mut missing_anchor = None;
                if let Some(pending) = tab.pending_anchor.take() {
                    let editor = tab.editor.read(cx);
                    if pending.revision == editor.buffer.revision()
                        && pending.cursor == editor.buffer.cursor()
                        && !tab.navigate_anchor(&pending.fragment, cx)
                    {
                        missing_anchor = Some(pending.fragment);
                    }
                }
                tab.prepared_revision = revision;
                if !prepared.analysis_complete() {
                    tab.requested_revision = u64::MAX;
                }
                for tab in &mut this.tabs {
                    tab.link_style_revision = (u64::MAX, u64::MAX);
                }
                this.preview_revision = None;
                if let Some(fragment) = missing_anchor {
                    this.message(
                        format!("Link target #{fragment} was not found in this note"),
                        false,
                        cx,
                    );
                }
                cx.notify();
            });
        }));
    }
    pub(super) fn reload_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.checkpointing {
            self.message("Finishing automatic save; try again in a moment", false, cx);
            return;
        }
        let Some(vault) = self.vault.clone() else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        let editor_id = tab.editor.entity_id();
        let revision = tab.editor.read(cx).buffer.revision();
        let path = tab.path.clone();
        tab._disk_reload = Some(cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                let text = vault.read(&path)?;
                let buffer = Buffer::new(opalglass::document::normalize(&text));
                let prepared = Arc::new(Prepared::new(buffer.text()));
                Ok::<_, std::io::Error>((text, buffer, prepared))
            }).await;
            let _ = this.update(cx, |this, cx| {
                let Some(tab) = this.tabs.iter_mut().find(|t| t.editor.entity_id() == editor_id) else { return; };
                if tab.editor.read(cx).buffer.revision() != revision {
                    this.message("The note was edited while reloading. Your edits are preserved; reload again or save a copy.", true, cx);
                    return;
                }
                match result {
                    Ok((text, mut buffer, prepared)) => {
                        let cursor = tab.editor.read(cx).buffer.cursor();
                        buffer.move_to(cursor, false);
                        tab.external_changed = false;
                        tab.baseline = text;
                        tab.baseline_normalized = buffer.snapshot();
                        let revision = buffer.revision();
                        tab.prepare_cancelled.store(true, Ordering::Relaxed);
                        tab._prepare = None;
                        tab.prepared_revision = revision;
                        tab.requested_revision = revision;
                        tab.dirty_cache.set((revision, false));
                        tab.install_prepared(prepared.clone(), false);
                        tab.editor.update(cx, |editor, cx| {
                            editor.set_buffer(buffer, cx);
                        });

                        this.revision += 1;
                        if let (Some(dir), Some(vault)) = (&this.state_directory, &this.vault) {
                            let _ = opalglass::state::Recovery::remove(dir, &vault.root, &tab.path);
                        }
                        this.message("Reloaded from disk", false, cx);
                    }
                    Err(error) => this.message(format!("Could not reload note: {error}"), true, cx),
                }
            });
        }));
    }
    pub(super) fn focus_document(&self, window: &mut Window, cx: &App) {
        self.tabs_scroll.scroll_to_item(self.active);
        if self.mode != ViewMode::Preview
            && let Some(tab) = self.tabs.get(self.active)
        {
            tab.editor.read(cx).focus.focus(window);
        } else {
            self.focus.focus(window);
        }
    }
    pub(super) fn toggle_live_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let started = Instant::now();
        self.live_preview = !self.live_preview;
        for tab in &self.tabs {
            tab.editor.update(cx, |editor, cx| {
                editor.set_live_preview(self.live_preview, cx)
            });
        }
        self.focus_document(window, cx);
        cx.notify();
        self.log_performance(
            "Change editor",
            if self.live_preview {
                "Live Preview"
            } else {
                "Raw Source"
            }
            .into(),
            started,
            cx,
        );
    }
    pub(super) fn set_mode(&mut self, mode: ViewMode, window: &mut Window, cx: &mut Context<Self>) {
        let started = Instant::now();
        self.mode = mode;
        self.split_drag_offset = None;
        self.focus_document(window, cx);
        cx.notify();
        self.log_performance(
            "Change view",
            match mode {
                ViewMode::Source => "Source",
                ViewMode::Split => "Split",
                ViewMode::Preview => "Reading",
            }
            .into(),
            started,
            cx,
        );
    }
    pub(super) fn filter_tag(&mut self, tag: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.navigation.hidden = false;
        self.search
            .update(cx, |editor, cx| editor.set_text(&format!("tag:{tag}"), cx));
        self.search.read(cx).focus.focus(window);
        cx.notify();
    }
    pub(super) fn follow(&mut self, target: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        self.follow_reading(
            reading_context::ReadingContext::host(tab),
            target,
            window,
            cx,
        );
    }
}
