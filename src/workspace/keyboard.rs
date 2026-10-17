// Workspace keyboard dispatch.
use super::*;

impl Workspace {
    pub(super) fn key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.managing {
            cx.stop_propagation();
            return;
        }
        let command = (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
            && !event.keystroke.modifiers.alt;
        let shift = event.keystroke.modifiers.shift;
        let key = event.keystroke.key.as_str();
        if self.replace_panel.is_some() {
            if key == "escape" {
                self.replace_panel = None;
                self._replace = None;
                self.focus_document(window, cx);
                cx.notify();
                cx.stop_propagation();
            }
            return;
        }
        if command && shift && key == "h" {
            self.open_replace(window, cx);
            cx.stop_propagation();
            return;
        }
        if self.mentions.is_some() {
            if key == "escape" {
                self.mentions = None;
                self._mentions = None;
                self.focus_document(window, cx);
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        if let Some(panel) = &self.history_panel {
            match key {
                "escape" => {
                    self.history_panel = None;
                    self._history = None;
                    self.focus_document(window, cx);
                    cx.notify();
                }
                "up" if panel.active > 0 => self.select_history(panel.active - 1, cx),
                "down" if panel.active + 1 < panel.versions.len() => {
                    self.select_history(panel.active + 1, cx)
                }
                "enter" => self.restore_history(window, cx),
                _ => (),
            }
            cx.stop_propagation();
            return;
        }
        if self.comparison.is_some() {
            if key == "escape" {
                self.comparison = None;
                self.focus_document(window, cx);
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        if self.properties_panel.is_some() {
            if key == "escape" {
                self.properties_panel = None;
                self.focus_document(window, cx);
                cx.notify();
                cx.stop_propagation();
            } else if command && key == "enter" {
                self.apply_properties(window, cx);
                cx.stop_propagation();
            }
            return;
        }
        if self.picker.is_some() {
            self.update_picker(cx);
            let count = self.picker_items.len();
            match key {
                "escape" => {
                    self.picker = None;
                    self.focus_document(window, cx);
                }
                "down" if count > 0 => self.picker_selected = (self.picker_selected + 1) % count,
                "up" if count > 0 => {
                    self.picker_selected = (self.picker_selected + count - 1) % count
                }
                "enter" => self.accept_picker(window, cx),
                _ => return,
            }
            self.picker_scroll
                .scroll_to_item(self.picker_selected, ScrollStrategy::Center);
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if self.settings_open {
            if key == "escape" {
                self.settings_open = false;
                self.focus_document(window, cx);
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        if key == "escape" && cx.has_active_drag() {
            cx.stop_active_drag(window);
            self.tab_drop = None;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.sort_menu.is_some() {
            match key {
                "escape" => {
                    self.sort_menu = None;
                    self.focus_document(window, cx);
                }
                "down" => self.sort_selected = (self.sort_selected + 1) % SortOrder::ALL.len(),
                "up" => {
                    self.sort_selected =
                        (self.sort_selected + SortOrder::ALL.len() - 1) % SortOrder::ALL.len()
                }
                "enter" | "space" => self.set_sort(SortOrder::ALL[self.sort_selected], window, cx),
                _ => (),
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.explorer_menu.is_some() {
            let action = if command && key == "n" {
                Some(if shift { Modal::NewFolder } else { Modal::New })
            } else if !command && key == "f2" {
                Some(Modal::Rename)
            } else {
                None
            };
            if let Some(action) = action {
                self.run_explorer_action(action, window, cx);
                cx.stop_propagation();
                return;
            }
            let actions = self.explorer_actions();
            let menu = self.explorer_menu.as_mut().unwrap();
            match key {
                "escape" => {
                    self.explorer_menu = None;
                    self.sort_menu = None;
                }
                "down" => menu.selected = (menu.selected + 1) % actions.len(),
                "up" => menu.selected = (menu.selected + actions.len() - 1) % actions.len(),
                "enter" | "space" => {
                    let action = actions[menu.selected].0;
                    self.run_explorer_action(action, window, cx);
                }
                _ => (),
            }
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if self.modal != Modal::None {
            if self.modal == Modal::Move && matches!(key, "up" | "down") && !command {
                self.update_move_results(cx);
                let count = self.move_results.len();
                if count > 0 {
                    self.move_selected = if key == "down" {
                        (self.move_selected + 1) % count
                    } else {
                        (self.move_selected + count - 1) % count
                    };
                    self.move_scroll
                        .scroll_to_item(self.move_selected, ScrollStrategy::Center);
                    cx.notify();
                }
                cx.stop_propagation();
                return;
            }
            match (command, key) {
                (false, "escape") => self.dismiss(window, cx),
                (false, "enter") => self.submit(window, cx),
                (true, _) => (),
                _ => return,
            }
            // Editing shortcuts were already handled by the dialog's input.
            // Keep workspace commands from mutating the document behind it.
            cx.stop_propagation();
            return;
        }
        if event.keystroke.modifiers.alt
            && (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
        {
            match key {
                "d" => self.open_daily_note(window, cx),
                "t" => self.show_picker(PickerMode::Templates, window, cx),
                _ => return,
            }
            cx.stop_propagation();
            return;
        }
        if event.keystroke.modifiers.alt && matches!(key, "left" | "right") {
            self.visit_history(key == "right", window, cx);
            cx.stop_propagation();
            return;
        }
        if self.find.open && !command {
            if key == "escape" {
                self.close_find(window, cx);
                cx.stop_propagation();
                return;
            }
            if key == "f3" || (key == "enter" && self.find.input.read(cx).focus.is_focused(window))
            {
                self.next_find(shift, cx);
                cx.stop_propagation();
                return;
            }
        }
        if !command && key == "delete" && self.tree_focus.is_focused(window) {
            self.show_modal(Modal::Delete, window, cx);
            cx.stop_propagation();
            return;
        }
        if !command && key == "f2" {
            if !self.tree_focus.is_focused(window) {
                self.navigation.selected = self.tabs.get(self.active).map(|t| t.path.clone());
            }
            self.show_inline(Modal::Rename, window, cx);
            cx.stop_propagation();
            return;
        }
        if !command && shift && key == "f10" && self.tree_focus.is_focused(window) {
            self.open_explorer_menu(
                self.navigation.selected.clone(),
                point(px(24.), px(160.)),
                window,
                cx,
            );
            cx.stop_propagation();
            return;
        }
        if !command && self.tree_key(event, window, cx) {
            cx.stop_propagation();
            return;
        }
        if command {
            match key {
                "b" if shift => self.reveal_active(window, cx),
                "b" => {
                    self.navigation.hidden = !self.navigation.hidden;
                    self.focus_document(window, cx);
                    cx.notify();
                }
                "s" if shift => self.show_modal(Modal::Copy, window, cx),
                "s" => {
                    self.save_tab(self.active, cx);
                }
                "n" if shift => self.show_inline(Modal::NewFolder, window, cx),
                "n" => self.show_inline(Modal::New, window, cx),
                "tab" if !self.tabs.is_empty() => {
                    let count = self.tabs.len();
                    self.activate_tab(
                        (self.active + if shift { count - 1 } else { 1 }) % count,
                        window,
                        cx,
                    );
                }
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" if !self.tabs.is_empty() => {
                    let index = if key == "9" {
                        self.tabs.len() - 1
                    } else {
                        key.parse::<usize>().unwrap() - 1
                    };
                    self.activate_tab(index, window, cx);
                }
                "," => {
                    self.settings_open = true;
                    self.focus.focus(window);
                    cx.notify();
                }
                "o" if shift => {
                    self.outline_open = !self.outline_open;
                    cx.notify();
                }
                "o" => self.pick_vault(window, cx),
                "f" if !shift => self.open_find(window, cx),
                "p" if shift => self.show_picker(PickerMode::Commands, window, cx),
                "d" if shift => self.toggle_bookmark(cx),
                "p" | "f" if self.vault.is_some() => {
                    self.navigation.hidden = false;
                    self.search.update(cx, |editor, cx| {
                        editor.buffer.select(0, editor.buffer.text().len());
                        cx.notify();
                    });
                    self.search.read(cx).focus.focus(window);
                }
                "w" => self.close_tab(window, cx),
                "e" if shift => self.toggle_live_preview(window, cx),
                "e" => {
                    let mode = match self.mode {
                        ViewMode::Source => ViewMode::Split,
                        ViewMode::Split => ViewMode::Preview,
                        ViewMode::Preview => ViewMode::Source,
                    };
                    self.set_mode(mode, window, cx);
                }
                "q" => {
                    if self.save_all_in(window, cx) {
                        cx.quit();
                    }
                }
                _ => return,
            }
        } else if key == "escape" {
            self.dismiss(window, cx);
        } else if matches!(key, "up" | "down")
            && self.search.read(cx).focus.is_focused(window)
            && !self.search_results.is_empty()
        {
            if key == "down" {
                self.search_selected =
                    (self.search_selected + 1).min(self.search_results.len() - 1);
            } else {
                self.search_selected = self.search_selected.saturating_sub(1);
            }
            self.results_scroll
                .scroll_to_item(self.search_selected, ScrollStrategy::Center);
            cx.stop_propagation();
            cx.notify();
        } else if key == "enter" && self.search.read(cx).focus.is_focused(window) {
            self.update_search(window, cx);
            if self.searching || (self.search_results.is_empty() && !self.search_ready()) {
                self.open_search_result = true;
            } else {
                self.open_first_search_result(window, cx);
            }
        } else {
            return;
        }
        cx.stop_propagation();
    }
}
