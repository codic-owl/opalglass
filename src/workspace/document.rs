// Document preparation, link styles, reload and pane synchronization.
use super::*;

impl Workspace {
    pub(super) fn update_link_styles(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let revision = (tab.prepared_revision, self.revision);
        // Resolve only when the document analysis or vault index changes. Never
        // apply offsets from an older analysis to a buffer with intervening edits.
        if tab.link_style_revision == revision
            || tab.prepared_revision != tab.editor.read(cx).buffer.revision()
        {
            return;
        }
        let Some(vault) = &self.vault else { return };
        let links = tab
            .prepared
            .links
            .iter()
            .map(|link| link.range.clone())
            .collect();
        let missing = tab
            .prepared
            .links
            .iter()
            .filter(|link| {
                !external_link(&link.target) && vault.resolve(&tab.path, &link.target).is_none()
            })
            .map(|link| link.range.clone())
            .collect();
        let mut code_blocks: Vec<std::ops::Range<usize>> =
            tab.prepared.frontmatter.iter().cloned().collect();
        for row in &tab.prepared.rows {
            if matches!(row.kind, Kind::Code) {
                if let Some(last) = code_blocks.last_mut()
                    && last.end == row.source.start
                {
                    last.end = row.source.end;
                } else {
                    code_blocks.push(row.source.clone());
                }
            }
        }
        tab.link_style_revision = revision;
        let syntax = tab.prepared.source_syntax.clone();
        tab.editor.update(cx, |editor, cx| {
            editor.set_code_blocks(code_blocks);
            editor.source_syntax = syntax;
            editor.set_source_links(links, missing);
            cx.notify();
        });
    }
    pub(super) fn link_highlight(&self, target: &str) -> HighlightStyle {
        let attachment = self
            .vault
            .as_ref()
            .zip(self.tabs.get(self.active))
            .is_some_and(|(vault, tab)| {
                opalglass::attachments::resolve(&vault.root, &tab.path, target).is_ok()
            });
        let missing = !attachment
            && !target.starts_with(markdown::TAG_TARGET_PREFIX)
            && !target.starts_with(opalglass::footnotes::TARGET_PREFIX)
            && !target.starts_with(opalglass::footnotes::BACK_PREFIX)
            && !external_link(target)
            && self.tabs.get(self.active).is_some_and(|tab| {
                self.vault.as_ref().is_some_and(|vault| {
                    vault.resolve(&tab.path, target).is_none()
                        && vault.resolve_url(&tab.path, target).is_none()
                })
            });
        HighlightStyle {
            color: Some(rgb(if missing { MISSING_LINK } else { ACCENT }).into()),
            underline: Some(if missing {
                missing_link_underline()
            } else {
                UnderlineStyle {
                    thickness: px(1.),
                    color: Some(rgb(0x786caa).into()),
                    wavy: false,
                }
            }),
            ..Default::default()
        }
    }
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
        let visible_index = tab.preview_projection.list_index(index);
        let fraction = if tab.preview_projection.row(visible_index) == index {
            fraction
        } else {
            0.
        };
        let index = visible_index;
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
                    .map(Arc::new)
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
                let offset = tab.preview_scroll.logical_scroll_top();
                tab.install_prepared(prepared.clone(), tab.prepared_revision == revision);
                if let Some(pending) = tab.pending_preview.take() {
                    tab.restore_preview_offset(pending);
                }
                if let Some(heading) = tab.pending_heading.take()
                    && offset.item_ix == 0
                    && offset.offset_in_item == px(0.)
                    && let Some((_, index)) = prepared
                        .headings
                        .iter()
                        .find(|(title, _)| markdown::heading_matches(title, &heading))
                {
                    tab.reveal_preview_row(*index);
                }
                tab.prepared_revision = revision;
                if !prepared.analysis_complete() {
                    tab.requested_revision = u64::MAX;
                }
                tab.link_style_revision = (u64::MAX, u64::MAX);
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
        if let Some(number) = target
            .strip_prefix(opalglass::footnotes::TARGET_PREFIX)
            .and_then(|n| n.parse().ok())
        {
            if let Some(tab) = self.tabs.get_mut(self.active)
                && let Some(row) = tab.prepared.footnotes.target_row(number)
            {
                let byte = tab.prepared.rows[row].source.start;
                tab.editor.update(cx, |e, cx| e.jump_to(byte, cx));
                tab.reveal_preview_row(row);
                self.preview_pane.update(cx, |_, cx| cx.notify());
            }
            return;
        }
        if let Some(byte) = target
            .strip_prefix(opalglass::footnotes::BACK_PREFIX)
            .and_then(|n| n.parse().ok())
        {
            if let Some(tab) = self.tabs.get_mut(self.active) {
                let row = tab.prepared.row_at_source(byte);
                tab.editor.update(cx, |e, cx| e.jump_to(byte, cx));
                tab.reveal_preview_row(row);
                self.preview_pane.update(cx, |_, cx| cx.notify());
            }
            return;
        }
        if let Some(tag) = target.strip_prefix(markdown::TAG_TARGET_PREFIX) {
            self.filter_tag(tag, window, cx);
            return;
        }
        if let (Some(vault), Some(tab)) = (&self.vault, self.tabs.get(self.active)) {
            if let Ok(path) = opalglass::attachments::resolve(&vault.root, &tab.path, &target) {
                cx.open_with_system(&path);
                return;
            }
            if let Some(path) = vault
                .resolve(&tab.path, &target)
                .or_else(|| vault.resolve_url(&tab.path, &target))
            {
                self.open_note_at(
                    path,
                    target.split_once('#').map(|(_, h)| h.to_owned()),
                    window,
                    cx,
                );
            } else {
                self.show_modal(Modal::New, window, cx);
                self.input.update(cx, |editor, cx| {
                    editor.set_text(target.split('#').next().unwrap_or(&target), cx)
                });
                self.message(
                    "Link target is missing or ambiguous. Choose a path for the new note.",
                    false,
                    cx,
                );
            }
        }
    }
}
