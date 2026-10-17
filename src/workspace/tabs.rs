// Saving, closing, activating and reordering tabs.
use super::*;

impl Workspace {
    pub(super) fn save_tab(&mut self, index: usize, cx: &mut Context<Self>) -> bool {
        if self.checkpointing {
            self.message("Finishing automatic save; try again in a moment", false, cx);
            return false;
        }
        let started = Instant::now();
        let Some(tab) = self.tabs.get(index) else {
            return true;
        };
        if !tab.dirty(cx) {
            if let (Some(dir), Some(vault)) = (&self.state_directory, &self.vault) {
                let _ = opalglass::state::Recovery::remove(dir, &vault.root, &tab.path);
            }
            return true;
        }
        let target = tab.path.display().to_string();
        let text = tab.disk_text(cx);
        let Some(vault) = &mut self.vault else {
            return false;
        };
        match vault.save(&tab.path, &tab.baseline, &text) {
            Ok(()) => {
                self.revision += 1;
                self.tabs[index].external_changed = false;
                self.tabs[index].baseline = text;
                self.tabs[index].baseline_normalized =
                    self.tabs[index].editor.read(cx).buffer.snapshot();
                self.tabs[index]
                    .dirty_cache
                    .set((self.tabs[index].editor.read(cx).buffer.revision(), false));
                if let Some(dir) = &self.state_directory {
                    let _ = opalglass::state::Recovery::remove(
                        dir,
                        &vault.root,
                        &self.tabs[index].path,
                    );
                }
                self.message("Saved to disk", false, cx);
                self.log_performance("Save note", target, started, cx);
                true
            }
            Err(error) => {
                if matches!(error, opalglass::vault::SaveError::Conflict) {
                    self.tabs[index].external_changed = true;
                }
                self.message(error.to_string(), true, cx);
                self.log_performance("Save failed", target, started, cx);
                false
            }
        }
    }
    pub fn save_all(&mut self, cx: &mut Context<Self>) -> bool {
        for index in 0..self.tabs.len() {
            if !self.save_tab(index, cx) {
                self.active = index;
                cx.notify();
                return false;
            }
        }
        true
    }
    pub fn save_all_in(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.checkpointing {
            self.message("Finishing automatic save; try again in a moment", false, cx);
            return false;
        }
        if self.managing {
            self.message("Finishing the move and updating links…", false, cx);
            return false;
        }
        self.persist_navigation(cx);
        if self.save_all(cx) {
            return true;
        }
        self.note_generation += 1;
        self._note_load = None;
        self.note_loading = false;
        // A failed quit or vault switch must direct subsequent input to the
        // conflicting note, including when another tab or dialog had focus.
        self.dismiss(window, cx);
        false
    }
    pub(super) fn close_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tab_at(self.active, window, cx);
    }
    pub(super) fn close_tab_at(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let started = Instant::now();
        if self.managing || index >= self.tabs.len() {
            return;
        }
        let target = self.tabs[index].path.display().to_string();
        self.modal = Modal::None;
        self.inline_parent = None;
        self.explorer_menu = None;
        self.sort_menu = None;
        if !self.save_tab(index, cx) {
            self.activate_tab(index, window, cx);
            self.log_performance("Close blocked", target, started, cx);
            return;
        }
        self.note_generation += 1;
        self._note_load = None;
        self.note_loading = false;
        if self.active == index {
            self.remember_departure(cx);
        }
        self.tabs.remove(index);
        if index < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        self.focus_document(window, cx);
        cx.notify();
        self.log_performance("Close tab", target, started, cx);
    }
    pub(super) fn activate_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let started = Instant::now();
        if self.managing || index >= self.tabs.len() {
            return;
        }
        if self.active != index {
            self.remember_departure(cx);
        }
        let action = if self.active == index {
            "Tab already open"
        } else {
            "Switch tab"
        };
        let changed = self.active != index
            || self.modal != Modal::None
            || self.inline_parent.is_some()
            || self.explorer_menu.is_some();
        self.note_generation += 1;
        self._note_load = None;
        self.note_loading = false;
        self.active = index;
        self.modal = Modal::None;
        self.inline_parent = None;
        self.explorer_menu = None;
        self.sort_menu = None;
        self.apply_history_position(cx);
        self.focus_document(window, cx);
        if changed {
            cx.notify();
        }
        self.log_performance(
            action,
            self.tabs[index].path.display().to_string(),
            started,
            cx,
        );
    }
    pub(super) fn reorder_tab(&mut self, editor: EntityId, before: usize, cx: &mut Context<Self>) {
        let started = Instant::now();
        self.tab_drop = None;
        if self.managing {
            return;
        }
        let Some(from) = self
            .tabs
            .iter()
            .position(|t| t.editor.entity_id() == editor)
        else {
            return;
        };
        let active = self.tabs[self.active].editor.entity_id();
        let tab = self.tabs.remove(from);
        let to = before
            .saturating_sub(usize::from(from < before))
            .min(self.tabs.len());
        self.tabs.insert(to, tab);
        self.tabs_scroll.scroll_to_item(to);
        self.active = self
            .tabs
            .iter()
            .position(|t| t.editor.entity_id() == active)
            .unwrap();
        cx.notify();
        self.log_performance(
            "Reorder tab",
            self.tabs[to].path.display().to_string(),
            started,
            cx,
        );
    }
}

impl Workspace {
    pub(super) fn render_tabs(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let mut tabs = div()
            .id("tabs")
            .track_scroll(&self.tabs_scroll)
            .overflow_x_scroll()
            .h(px(43.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .bg(rgb(0x181c25))
            .border_b_1()
            .border_color(rgb(LINE));
        for (index, tab) in self.tabs.iter().enumerate() {
            let drop_after = self.tab_drop == Some(index + 1);
            let title = tab
                .path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let drag = TabDrag {
                editor: tab.editor.entity_id(),
                title: title.clone(),
            };
            tabs = tabs.child(
                div()
                    .id(("tab", index))
                    .debug_selector(move || format!("tab-{index}"))
                    .h_full()
                    .min_w(px(120.))
                    .px_3()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_l_2()
                    .border_r_1()
                    .border_color(rgb(LINE))
                    .cursor_pointer()
                    .bg(rgb(if index == self.active { BG } else { 0x181c25 }))
                    .text_sm()
                    .text_color(rgb(if index == self.active { TEXT } else { MUTED }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .max_w(px(180.))
                            .truncate()
                            .child(format!(
                                "{}{}",
                                title,
                                if tab.dirty(cx) { "  •" } else { "" }
                            )),
                    )
                    .tooltip({
                        let path = tab.path.display().to_string();
                        move |_, cx| cx.new(|_| PathTooltip(path.clone())).into()
                    })
                    .on_drag(drag, |drag, _, _, cx| {
                        cx.new(|_| PathTooltip(drag.title.clone()))
                    })
                    .drag_over::<TabDrag>(move |style, _, _, _| {
                        if drop_after {
                            style.border_l_0().border_r_2().border_color(rgb(ACCENT))
                        } else {
                            style.border_l_2().border_r_0().border_color(rgb(ACCENT))
                        }
                    })
                    .on_drag_move(cx.listener(
                        move |this, event: &DragMoveEvent<TabDrag>, _, cx| {
                            if event.bounds.contains(&event.event.position) {
                                let before = index
                                    + usize::from(
                                        event.event.position.x >= event.bounds.center().x,
                                    );
                                if this.tab_drop != Some(before) {
                                    this.tab_drop = Some(before);
                                    cx.notify();
                                }
                            }
                        },
                    ))
                    .on_drop(cx.listener(move |this, drag: &TabDrag, window, cx| {
                        this.reorder_tab(drag.editor, this.tab_drop.unwrap_or(index), cx);
                        this.focus_document(window, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(move |this, _, window, cx| {
                            this.close_tab_at(index, window, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.activate_tab(index, window, cx);
                            window.prevent_default();
                        }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        // Mouse press already activated this tab. Keep click
                        // activation for keyboard/accessibility without logging twice.
                        if this.managing || index >= this.tabs.len() {
                            return;
                        }
                        if this.active != index {
                            this.activate_tab(index, window, cx);
                        }
                        this.reveal_active(window, cx);
                        this.focus_document(window, cx);
                    }))
                    .child(
                        div()
                            .id(("close-tab", index))
                            .debug_selector(move || format!("close-tab-{index}"))
                            .size(px(22.))
                            .rounded_sm()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(rgb(MUTED))
                            .hover(|s| s.bg(rgb(0x403748)).text_color(rgb(TEXT)))
                            .child("×")
                            .tooltip(|_, cx| {
                                cx.new(|_| PathTooltip("Close tab · Ctrl W".into())).into()
                            })
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.close_tab_at(index, window, cx);
                                cx.stop_propagation();
                            })),
                    ),
            );
        }
        let end = self.tabs.len();
        tabs = tabs.child(
            div()
                .id("tabs-end")
                .debug_selector(|| "tabs-end".into())
                .min_w(px(36.))
                .flex_1()
                .h_full()
                .border_l_2()
                .border_color(rgb(LINE))
                .drag_over::<TabDrag>(|style, _, _, _| style.border_color(rgb(ACCENT)))
                .on_drop(cx.listener(move |this, drag: &TabDrag, window, cx| {
                    this.reorder_tab(drag.editor, end, cx);
                    this.focus_document(window, cx);
                })),
        );
        tabs
    }
}
