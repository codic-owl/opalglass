// Explorer menus, inline naming, drag/drop, moves and recoverable trash.
use super::*;

impl Workspace {
    pub(super) fn open_explorer_menu(
        &mut self,
        target: Option<PathBuf>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.managing || self.vault.is_none() {
            return;
        }
        self.dismiss(window, cx);
        self.navigation.selected = target.clone();
        self.explorer_menu = Some(ExplorerMenu {
            target,
            position,
            selected: 0,
        });
        self.tree_focus.focus(window);
        cx.notify();
    }
    pub(super) fn explorer_actions(&self) -> Vec<(Modal, &'static str, &'static str)> {
        let Some(menu) = &self.explorer_menu else {
            return Vec::new();
        };
        let folder = menu
            .target
            .as_ref()
            .is_none_or(|p| self.vault.as_ref().is_some_and(|v| v.folders.contains(p)));
        let mut actions = Vec::new();
        if folder {
            actions.push((Modal::New, "New note", "Ctrl N"));
            actions.push((Modal::NewFolder, "New folder", "Ctrl Shift N"));
        }
        if menu.target.is_some() {
            actions.push((Modal::Rename, "Rename…", "F2"));
            actions.push((
                Modal::Move,
                if folder {
                    "Move folder to…"
                } else {
                    "Move file to…"
                },
                "",
            ));
        }
        if menu.target.is_some() {
            actions.push((Modal::Delete, "Move to trash…", "Delete"));
        }
        actions.push((Modal::RestoreTrash, "Restore last deleted item", ""));
        actions
    }
    pub(super) fn run_explorer_action(
        &mut self,
        action: Modal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.explorer_menu.take() else {
            return;
        };
        self.navigation.selected = menu.target;
        if action == Modal::RestoreTrash {
            self.change_trash(None, window, cx);
        } else if matches!(action, Modal::Move | Modal::Delete) {
            self.show_modal(action, window, cx);
        } else {
            self.show_inline(action, window, cx);
        }
    }
    pub(super) fn show_inline(
        &mut self,
        action: Modal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_modal(action, window, cx);
        if self.modal != action {
            return;
        }
        self.navigation.hidden = false;
        self.search.update(cx, |editor, cx| editor.set_text("", cx));
        let parent = if action == Modal::Rename {
            let target = self.modal_target.clone().unwrap();
            self.reveal_path(&target);
            target
                .parent()
                .unwrap_or(std::path::Path::new(""))
                .to_path_buf()
        } else {
            let parent = self.selected_folder();
            if !parent.as_os_str().is_empty() {
                self.navigation.expanded.insert(parent.clone());
                self.reveal_path(&parent);
                self.tree.flatten(&self.navigation.expanded);
            }
            parent
        };
        let initial = if action == Modal::Rename {
            let path = self.modal_target.as_ref().unwrap();
            if self
                .vault
                .as_ref()
                .is_some_and(|v| v.folders.contains(path) || v.files.contains(path))
            {
                path.file_name()
            } else {
                path.file_stem()
            }
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
        } else {
            String::new()
        };
        self.inline_parent = Some(parent);
        self.input.update(cx, |editor, cx| {
            editor.set_text(&initial, cx);
            editor.buffer.select(0, initial.len());
        });
        let index = if action == Modal::Rename {
            self.tree
                .visible
                .iter()
                .position(|e| Some(&e.path) == self.modal_target.as_ref())
                .unwrap_or(0)
        } else {
            self.inline_insert_index().unwrap_or(0)
        };
        self.notes_scroll
            .scroll_to_item(index, ScrollStrategy::Center);
        self.input.read(cx).focus.focus(window);
        cx.notify();
    }
    pub(super) fn inline_insert_index(&self) -> Option<usize> {
        let parent = self.inline_parent.as_ref()?;
        if !matches!(self.modal, Modal::New | Modal::NewFolder) {
            return None;
        }
        Some(if parent.as_os_str().is_empty() {
            0
        } else {
            self.tree
                .visible
                .iter()
                .position(|e| &e.path == parent)
                .map_or(0, |i| i + 1)
        })
    }
    pub(super) fn inline_row(&self) -> AnyElement {
        let depth = self
            .inline_parent
            .as_ref()
            .map_or(0, |p| p.components().count());
        div()
            .h(px(32.))
            .pl(px(8. + depth as f32 * 16.))
            .pr_2()
            .child(
                div()
                    .debug_selector(|| "entry-name".into())
                    .h_full()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(if self.error { 0xf1b18e } else { ACCENT }))
                    .bg(rgb(BG))
                    .child(self.input.clone()),
            )
            .into_any_element()
    }
    pub(super) fn can_move_to(&self, from: &std::path::Path, folder: &std::path::Path) -> bool {
        !self.managing
            && self.vault.as_ref().is_some_and(|v| {
                (folder.as_os_str().is_empty() || v.folders.iter().any(|p| p == folder))
                    && (v.folders.iter().any(|p| p == from)
                        || v.files.iter().any(|p| p == from)
                        || v.notes.iter().any(|n| n.path == from))
                    && from.parent() != Some(folder)
                    && !folder.starts_with(from)
            })
    }
    pub(super) fn drop_entry(
        &mut self,
        drag: &EntryDrag,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.vault.as_ref().is_none_or(|v| v.root != drag.root)
            || !self.can_move_to(&drag.path, &folder)
        {
            return;
        }
        let Some(name) = drag.path.file_name() else {
            return;
        };
        self.dismiss(window, cx);
        self.move_entry(drag.path.clone(), folder.join(name), window, cx);
    }
    pub(super) fn folder_drop_target(
        &self,
        el: Stateful<Div>,
        folder: PathBuf,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let entity = cx.entity().downgrade();
        let target = folder.clone();
        el.can_drop(move |value, _, cx| {
            value.downcast_ref::<EntryDrag>().is_some_and(|drag| {
                entity.upgrade().is_some_and(|this| {
                    let this = this.read(cx);
                    this.vault.as_ref().is_some_and(|v| v.root == drag.root)
                        && this.can_move_to(&drag.path, &target)
                })
            })
        })
        .drag_over::<EntryDrag>(|style, _, _, _| style.bg(rgb(0x403758)).text_color(rgb(ACCENT)))
        .on_drop(cx.listener(move |this, drag: &EntryDrag, window, cx| {
            this.drop_entry(drag, folder.clone(), window, cx);
            cx.stop_propagation();
        }))
    }
    pub(super) fn update_move_results(&mut self, cx: &App) {
        let query = self.input.read(cx).buffer.text().trim().to_lowercase();
        if self.move_query == query {
            return;
        }
        self.move_query = query;
        self.move_results = self
            .move_candidates
            .iter()
            .filter(|p| {
                let label = if p.as_os_str().is_empty() {
                    "/".into()
                } else {
                    p.display().to_string()
                };
                label.to_lowercase().contains(&self.move_query)
            })
            .cloned()
            .collect();
        self.move_selected = 0;
        self.move_scroll.scroll_to_item(0, ScrollStrategy::Top);
    }
    pub(super) fn choose_move_folder(
        &mut self,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self.modal_target.clone() else {
            return;
        };
        if !self.can_move_to(&from, &folder) {
            return;
        }
        let Some(name) = from.file_name() else { return };
        let to = folder.join(name);
        self.move_entry(from, to, window, cx);
    }
    pub(super) fn render_explorer_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let menu = self.explorer_menu.as_ref().unwrap();
        let mut content = div()
            .id("explorer-menu")
            .debug_selector(|| "explorer-menu".into())
            .w(px(250.))
            .p_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(LINE))
            .bg(rgb(PANEL))
            .shadow_lg()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.explorer_menu = None;
                cx.notify();
            }));
        for (index, (action, label, shortcut)) in self.explorer_actions().into_iter().enumerate() {
            content = content.child(
                div()
                    .id(("explorer-action", index))
                    .debug_selector(move || format!("explorer-action-{index}"))
                    .h(px(32.))
                    .px_3()
                    .rounded_sm()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_sm()
                    .cursor_pointer()
                    .when(index == menu.selected, |el| el.bg(rgb(0x343049)))
                    .hover(|s| s.bg(rgb(0x343049)))
                    .child(label)
                    .child(caption(shortcut))
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if let Some(menu) = &mut this.explorer_menu
                            && menu.selected != index
                        {
                            menu.selected = index;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run_explorer_action(action, window, cx);
                        cx.stop_propagation();
                    })),
            );
        }
        deferred(
            anchored()
                .position(menu.position)
                .snap_to_window_with_margin(px(8.))
                .child(content),
        )
        .with_priority(1)
        .into_any_element()
    }
    pub(super) fn render_move_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .absolute()
            .inset_0()
            .occlude()
            .bg(rgba(0x05070bb0))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(500.))
                    .p_5()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(PANEL))
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_lg().child("Move to folder"))
                    .child(
                        caption(
                            self.modal_target
                                .as_ref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default(),
                        )
                        .truncate(),
                    )
                    .child(
                        div()
                            .h(px(40.))
                            .border_1()
                            .border_color(rgb(ACCENT))
                            .rounded_md()
                            .bg(rgb(BG))
                            .child(self.input.clone()),
                    )
                    .child(caption("Search folders · ↑ ↓ to choose · Enter to move"))
                    .when(self.move_results.is_empty(), |el| {
                        el.child(caption("No matching destination folders"))
                    })
                    .child(
                        uniform_list(
                            "move-folders",
                            self.move_results.len(),
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .map(|index| {
                                        let path = this.move_results[index].clone();
                                        let label = if path.as_os_str().is_empty() {
                                            "/".into()
                                        } else {
                                            path.display().to_string()
                                        };
                                        div()
                                            .id(("move-folder", index))
                                            .debug_selector(move || format!("move-folder-{index}"))
                                            .h(px(34.))
                                            .px_3()
                                            .py_1()
                                            .rounded_md()
                                            .text_sm()
                                            .cursor_pointer()
                                            .when(index == this.move_selected, |el| {
                                                el.bg(rgb(0x343049))
                                            })
                                            .hover(|s| s.bg(rgb(0x303749)))
                                            .child(label)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.choose_move_folder(path.clone(), window, cx)
                                            }))
                                    })
                                    .collect()
                            }),
                        )
                        .h(px(240.))
                        .track_scroll(self.move_scroll.clone()),
                    )
                    .when(self.error, |el| {
                        el.child(
                            div()
                                .text_sm()
                                .text_color(rgb(0xf1b18e))
                                .child(self.status.clone()),
                        )
                    })
                    .child(
                        div().flex().justify_end().child(
                            button("cancel-move", "Cancel").on_click(
                                cx.listener(|this, _, window, cx| this.dismiss(window, cx)),
                            ),
                        ),
                    ),
            )
            .into_any_element()
    }
    pub(super) fn selected_folder(&self) -> PathBuf {
        let Some(path) = &self.navigation.selected else {
            return PathBuf::new();
        };
        if self
            .vault
            .as_ref()
            .is_some_and(|v| v.folders.contains(path))
        {
            path.clone()
        } else {
            path.parent().unwrap_or(std::path::Path::new("")).into()
        }
    }
    pub(super) fn change_trash(
        &mut self,
        deleted: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.managing || self.checkpointing {
            return;
        }
        if let Some(path) = &deleted {
            for index in 0..self.tabs.len() {
                if self.tabs[index].path.starts_with(path) && !self.save_tab(index, cx) {
                    self.activate_tab(index, window, cx);
                    return;
                }
            }
        }
        let Some(mut vault) = self.vault.clone() else {
            return;
        };
        self.cancel_indexing();
        self.note_generation += 1;
        self._note_load = None;
        self.note_loading = false;
        self.managing = true;
        self.revision += 1;
        self.focus.focus(window);
        self.message(
            if deleted.is_some() {
                "Moving to trash…"
            } else {
                "Restoring from trash…"
            },
            false,
            cx,
        );
        self._management = Some(cx.spawn_in(window, async move |this, cx| {
            let path = deleted.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let path = if let Some(path) = path {
                        vault.trash_entry(&path)?;
                        path
                    } else {
                        vault.restore_last_trashed()?
                    };
                    Ok::<_, std::io::Error>((vault, path))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.managing = false;
                match result {
                    Ok((vault, path)) => {
                        let active = this.tabs.get(this.active).map(|t| t.editor.entity_id());
                        if deleted.is_some() {
                            this.tabs.retain(|t| !t.path.starts_with(&path));
                            this.navigation.expanded.retain(|p| !p.starts_with(&path));
                            this.navigation.selected = None;
                        }
                        this.active = this
                            .tabs
                            .iter()
                            .position(|t| Some(t.editor.entity_id()) == active)
                            .unwrap_or(this.active.min(this.tabs.len().saturating_sub(1)));
                        this.vault = Some(vault);
                        this.revision += 1;
                        this.dismiss(window, cx);
                        if deleted.is_none() {
                            this.reveal_path(&path);
                        }
                        this.message(
                            format!(
                                "{} {}",
                                if deleted.is_some() {
                                    "Moved to trash:"
                                } else {
                                    "Restored:"
                                },
                                path.display()
                            ),
                            false,
                            cx,
                        );
                        this.persist_navigation(cx);
                    }
                    Err(error) => {
                        this.message(format!("Could not change trash: {error}"), true, cx)
                    }
                }
            });
        }));
    }
    pub(super) fn move_entry(
        &mut self,
        from: PathBuf,
        to: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.save_all_in(window, cx) {
            return;
        }
        let Some(mut vault) = self.vault.clone() else {
            return;
        };
        self.cancel_indexing();
        self.load_cancelled.store(true, Ordering::Relaxed);
        self.managing = true;
        self.load_generation += 1;
        self._load = None;
        self.loading = false;
        self.revision += 1;
        self.note_generation += 1;
        self._note_load = None;
        self.note_loading = false;
        // Any in-flight reload/analysis is superseded by this operation.
        for tab in &mut self.tabs {
            tab._disk_reload = None;
            tab.prepare_cancelled.store(true, Ordering::Relaxed);
            tab._prepare = None;
            tab.requested_revision = u64::MAX;
        }
        let open: Vec<_> = self
            .tabs
            .iter()
            .map(|t| (t.path.clone(), t.baseline.clone()))
            .collect();
        self.focus.focus(window);
        self.message("Moving and updating wiki links…", false, cx);
        self._management = Some(cx.spawn_in(window, async move |this, cx| {
            let source = from.clone();
            let target = to.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    vault.move_entry(&source, &target)?;
                    let ready: Vec<_> = open
                        .into_iter()
                        .map(|(path, baseline)| {
                            let path = opalglass::vault::moved_path(&path, &source, &target);
                            let replacement = vault
                                .notes
                                .iter()
                                .find(|n| n.path == path)
                                .filter(|n| !n.matches_text(&baseline))
                                .map(|_| {
                                    vault.read(&path).map(|text| {
                                        let buffer =
                                            Buffer::new(opalglass::document::normalize(&text));
                                        let prepared = Arc::new(Prepared::new(buffer.text()));
                                        (text, buffer, prepared)
                                    })
                                })
                                .transpose();
                            (path, replacement)
                        })
                        .collect();
                    Ok::<_, std::io::Error>((vault, ready))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.managing = false;
                match result {
                    Ok((vault, ready)) => {
                        let mut reload_error = None;
                        for (tab, (path, replacement)) in this.tabs.iter_mut().zip(ready) {
                            tab.path = path;
                            let replacement = match replacement {
                                Ok(replacement) => replacement,
                                Err(error) => {
                                    // The move already committed. Keep the tab
                                    // at its new path with its buffer intact.
                                    reload_error = Some(format!(
                                        "Moved, but could not reload {}: {error}",
                                        tab.path.display()
                                    ));
                                    None
                                }
                            };
                            if let Some((text, mut buffer, prepared)) = replacement {
                                buffer.move_to(tab.editor.read(cx).buffer.cursor(), false);
                                tab.baseline = text;
                                tab.baseline_normalized = buffer.snapshot();
                                let revision = buffer.revision();
                                tab.dirty_cache.set((revision, false));
                                tab.prepared_revision = revision;
                                tab.requested_revision = revision;
                                tab.install_prepared(prepared.clone(), false);
                                tab.editor.update(cx, |editor, cx| {
                                    editor.set_buffer(buffer, cx);
                                });
                            }
                        }
                        for location in this
                            .history_back
                            .iter_mut()
                            .chain(this.history_forward.iter_mut())
                        {
                            location.path =
                                opalglass::vault::moved_path(&location.path, &from, &to);
                        }
                        for path in &mut this.navigation.bookmarks {
                            *path = opalglass::vault::moved_path(path, &from, &to);
                        }
                        this.navigation.expanded = this
                            .navigation
                            .expanded
                            .iter()
                            .map(|p| opalglass::vault::moved_path(p, &from, &to))
                            .collect();
                        this.vault = Some(vault);
                        this.revision += 1;
                        this.reveal_path(&to);
                        this.dismiss(window, cx);
                        this.tree_focus.focus(window);
                        if let Some(error) = reload_error {
                            this.message(error, true, cx);
                        } else {
                            this.message("Moved; wiki links and open tabs updated", false, cx);
                        }
                    }
                    Err(error) => {
                        if this.modal == Modal::None {
                            this.tree_focus.focus(window);
                        } else {
                            this.input.read(cx).focus.focus(window);
                        }
                        this.message(format!("Could not move: {error}"), true, cx);
                    }
                }
            });
        }));
    }
}
