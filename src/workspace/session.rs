// Session restoration, autosave, recovery checkpoints and navigation persistence.
use super::*;

impl Workspace {
    pub(super) fn restore_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(vault) = self.vault.clone() else {
            return;
        };
        let directory = self.state_directory.clone();
        let saved = self.navigation.tabs.clone();
        let active = self.navigation.active_tab.clone();
        let generation = self.note_generation;
        let root = vault.root.clone();
        self.restoring = true;
        self._restore = Some(cx.spawn_in(window, async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    let mut seen = std::collections::HashSet::new();
                    saved
                        .into_iter()
                        .filter_map(|saved| {
                            if !seen.insert(saved.path.clone()) {
                                return None;
                            }
                            let (text, buffer) = opalglass::state::load_document(
                                &vault,
                                &saved.path,
                                directory.as_deref(),
                            )
                            .ok()?;
                            let prepared = Arc::new(Prepared::opening(buffer.text()));
                            Some((saved, text, buffer, prepared))
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.vault.as_ref().is_none_or(|v| v.root != root) {
                    return;
                }
                this.restoring = false;
                // User navigation takes precedence over startup restoration.
                if this.note_generation != generation {
                    return;
                }
                for (saved, text, buffer, prepared) in loaded {
                    this.install_tab(saved.path, text, buffer, prepared, window, cx);
                    let tab = this.tabs.last_mut().unwrap();
                    tab.editor.update(cx, |e, cx| {
                        e.restore_position(saved.anchor, saved.cursor, saved.source, cx)
                    });
                    let offset = ListOffset {
                        item_ix: saved.preview.row,
                        offset_in_item: px(saved.preview.offset),
                    };
                    if tab.prepared.complete {
                        tab.restore_preview_offset(offset);
                    } else {
                        tab.pending_preview = Some(offset);
                    }
                }
                this.active = this
                    .tabs
                    .iter()
                    .position(|t| Some(&t.path) == active.as_ref())
                    .unwrap_or(0);
                this.focus_document(window, cx);
                cx.notify();
            });
        }));
    }
    pub(super) fn checkpoint(&mut self, cx: &mut Context<Self>) {
        if self.checkpointing || self.managing || self.loading || self.restoring {
            return;
        }
        let Some(mut vault) = self.vault.clone() else {
            return;
        };
        // Test workspaces opt in by assigning a temporary state directory.
        let Some(directory) = self.state_directory.clone() else {
            return;
        };
        let autosave = self.preferences.autosave;
        let mut requests = Vec::new();
        self.checkpoints
            .retain(|id, _| self.tabs.iter().any(|t| t.editor.entity_id() == *id));
        for tab in &self.tabs {
            let id = tab.editor.entity_id();
            let key = (
                tab.editor.read(cx).buffer.revision(),
                autosave,
                tab.path.clone(),
            );
            if self.checkpoints.get(&id) == Some(&key) {
                continue;
            }
            let dirty = tab.dirty(cx);
            if dirty || self.checkpoints.contains_key(&id) {
                requests.push((
                    id,
                    tab.path.clone(),
                    tab.baseline.clone(),
                    dirty.then(|| tab.disk_text(cx)),
                ));
            }
            self.checkpoints.insert(id, key);
        }
        if requests.is_empty() {
            return;
        }
        self.checkpointing = true;
        let root = vault.root.clone();
        let started = Instant::now();
        self._checkpoint = Some(cx.spawn(async move |this, cx| {
            let (vault, results) = cx
                .background_executor()
                .spawn(async move {
                    let mut results = Vec::new();
                    for (id, path, baseline, text) in requests {
                        let result = (|| -> Result<Option<String>, String> {
                            let Some(text) = text else {
                                opalglass::state::Recovery::remove(&directory, &vault.root, &path)
                                    .map_err(|e| e.to_string())?;
                                return Ok(None);
                            };
                            opalglass::state::Recovery {
                                root: vault.root.clone(),
                                path: path.clone(),
                                baseline: baseline.clone(),
                                text: text.clone(),
                            }
                            .save(&directory)
                            .map_err(|e| format!("Could not save recovery: {e}"))?;
                            if !autosave {
                                return Ok(None);
                            }
                            vault
                                .save(&path, &baseline, &text)
                                .map_err(|e| e.to_string())?;
                            // Retaining a stale record is safe: load_document compares
                            // it with disk before offering recovered edits.
                            let _ =
                                opalglass::state::Recovery::remove(&directory, &vault.root, &path);
                            Ok(Some(text))
                        })();
                        results.push((id, path, baseline, result));
                    }
                    (vault, results)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.checkpointing = false;
                if this.vault.as_ref().is_none_or(|v| v.root != root) {
                    return;
                }
                let mut saved = 0;
                let mut errors = Vec::new();
                for (id, path, baseline, result) in results {
                    match result {
                        Ok(Some(text)) => {
                            if let Some(tab) = this.tabs.iter_mut().find(|t| {
                                t.editor.entity_id() == id
                                    && t.path == path
                                    && t.baseline == baseline
                            }) {
                                // Edits made during the write remain in the buffer
                                // and dirty against the snapshot that reached disk.
                                tab.baseline_normalized =
                                    Arc::new(opalglass::document::normalize(&text));
                                tab.baseline = text;
                                tab.dirty_cache.set((u64::MAX, true));
                            }
                            saved += 1;
                        }
                        Ok(None) => (),
                        Err(error) => {
                            this.checkpoints.remove(&id);
                            errors.push(format!("{}: {error}", path.display()));
                        }
                    }
                }
                if saved > 0 {
                    this.cancel_indexing();
                    this.vault = Some(vault);
                    this.revision += 1;
                }
                if !errors.is_empty() {
                    this.message(
                        format!("Automatic save/recovery: {}", errors.join("; ")),
                        true,
                        cx,
                    );
                } else if saved > 0 {
                    this.message(format!("Automatically saved {saved} note(s)"), false, cx);
                    this.log_performance("Autosave", format!("{saved} note(s)"), started, cx);
                }
                cx.notify();
            });
        }));
    }
    pub(super) fn persist_navigation(&mut self, cx: &mut Context<Self>) {
        if self.restoring {
            return;
        }
        self.navigation.session_initialized = true;
        self.navigation.tabs = self
            .tabs
            .iter()
            .map(|tab| {
                let editor = tab.editor.read(cx);
                let preview = tab
                    .pending_preview
                    .unwrap_or_else(|| tab.saved_preview_offset());
                navigation::SessionTab {
                    path: tab.path.clone(),
                    cursor: editor.buffer.cursor(),
                    anchor: editor.buffer.anchor(),
                    source: editor.saved_scroll(),
                    preview: navigation::ScrollPosition {
                        row: preview.item_ix,
                        offset: preview.offset_in_item.into(),
                    },
                }
            })
            .collect();
        self.navigation.active_tab = self.tabs.get(self.active).map(|t| t.path.clone());
        self.navigation.scroll_y = self.notes_scroll.0.borrow().base_handle.offset().y.into();
        if self.persisted_navigation.as_ref() == Some(&self.navigation) {
            return;
        }
        if let (Some(directory), Some(vault)) = (&self.state_directory, &self.vault)
            && let Err(error) = self.navigation.save(directory, &vault.root)
        {
            self.message(format!("Could not save sidebar state: {error}"), true, cx);
            return;
        }
        self.persisted_navigation = Some(self.navigation.clone());
    }
}
