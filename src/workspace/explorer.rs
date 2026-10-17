// File-tree state, keyboard navigation and sidebar rendering.
use super::*;

impl Workspace {
    pub(super) fn update_tree(&mut self) {
        if self.tree_revision != self.revision {
            if let Some(vault) = &self.vault {
                self.tree
                    .rebuild(vault, &self.navigation.expanded, self.navigation.sort);
            }
            self.tree_revision = self.revision;
        }
    }
    pub(super) fn toggle_folder(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.navigation.expanded.remove(&path) {
            self.navigation.expanded.insert(path);
        }
        self.tree.flatten(&self.navigation.expanded);
        cx.notify();
    }
    pub(super) fn sync_auto_reveal(&mut self) {
        if !self.navigation.auto_reveal {
            self.auto_revealed = None;
            return;
        }
        let path = self.tabs.get(self.active).map(|tab| tab.path.clone());
        if path != self.auto_revealed {
            self.auto_revealed = path.clone();
            if let Some(path) = path {
                // Follow navigation without stealing focus, clearing a search,
                // or reopening a sidebar the user deliberately collapsed.
                self.reveal_path(&path);
            }
        }
    }
    pub(super) fn set_sort(
        &mut self,
        order: SortOrder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigation.sort = order;
        self.sort_menu = None;
        self.tree_revision = u64::MAX;
        self.update_tree();
        self.notes_scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(0.)));
        self.focus_document(window, cx);
        cx.notify();
    }
    pub(super) fn reveal_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.tabs.get(self.active).map(|t| t.path.clone()) else {
            return;
        };
        self.navigation.hidden = false;
        self.search.update(cx, |editor, cx| editor.set_text("", cx));
        self.reveal_path(&path);
        self.tree_focus.focus(window);
        cx.notify();
    }
    pub(super) fn reveal_path(&mut self, path: &std::path::Path) {
        self.navigation.reveal(path);
        self.update_tree();
        self.tree.flatten(&self.navigation.expanded);
        if let Some(index) = self.tree.visible.iter().position(|e| e.path == path) {
            self.notes_scroll
                .scroll_to_item(index, ScrollStrategy::Center);
        }
    }
    pub(super) fn tree_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.tree_focus.is_focused(window) || !self.search_query.is_empty() {
            return false;
        }
        self.update_tree();
        if self.tree.visible.is_empty() {
            return false;
        }
        let index = self
            .navigation
            .selected
            .as_ref()
            .and_then(|p| self.tree.visible.iter().position(|e| &e.path == p))
            .unwrap_or(0);
        let entry = self.tree.visible[index].clone();
        let mut next = index;
        match event.keystroke.key.as_str() {
            "down" => {
                next = if self.navigation.selected.is_none() {
                    0
                } else {
                    (index + 1).min(self.tree.visible.len() - 1)
                }
            }
            "up" => next = index.saturating_sub(1),
            "home" => next = 0,
            "end" => next = self.tree.visible.len() - 1,
            "right" if entry.folder => {
                if self.navigation.expanded.contains(&entry.path) {
                    if self
                        .tree
                        .visible
                        .get(index + 1)
                        .is_some_and(|e| e.depth > entry.depth)
                    {
                        next += 1;
                    }
                } else {
                    self.toggle_folder(entry.path.clone(), cx);
                }
            }
            "left" => {
                if entry.folder && self.navigation.expanded.contains(&entry.path) {
                    self.toggle_folder(entry.path.clone(), cx);
                } else if let Some(parent) = entry
                    .path
                    .parent()
                    .and_then(|p| self.tree.visible.iter().position(|e| e.path == p))
                {
                    next = parent;
                }
            }
            "enter" | "space" if entry.folder => self.toggle_folder(entry.path.clone(), cx),
            "enter" => {
                self.open_note(entry.path, window, cx);
                return true;
            }
            _ => {
                let Some(text) = event
                    .keystroke
                    .key_char
                    .as_deref()
                    .filter(|s| !s.chars().any(char::is_control))
                else {
                    return false;
                };
                if self.typeahead_at.elapsed() > Duration::from_millis(800) {
                    self.typeahead.clear();
                }
                self.typeahead_at = std::time::Instant::now();
                self.typeahead.push_str(&text.to_lowercase());
                if let Some(found) = (1..=self.tree.visible.len())
                    .map(|n| (index + n) % self.tree.visible.len())
                    .find(|&i| {
                        self.tree.visible[i]
                            .name()
                            .to_lowercase()
                            .starts_with(&self.typeahead)
                    })
                {
                    next = found;
                }
            }
        }
        self.navigation.selected = Some(self.tree.visible[next].path.clone());
        self.notes_scroll
            .scroll_to_item(next, ScrollStrategy::Center);
        cx.notify();
        true
    }
    pub(super) fn explorer_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let enabled = self.vault.is_some() && !self.managing;
        let expanded = !self.navigation.expanded.is_empty();
        div()
            .debug_selector(|| "explorer-toolbar".into())
            .h(px(43.))
            .flex_shrink_0()
            .px_2()
            .flex()
            .items_center()
            .justify_end()
            .gap_1()
            .child(
                icon_button(
                    "explorer-new-note",
                    ToolbarIcon::NewNote,
                    "New note".into(),
                    enabled,
                )
                .when(enabled, |el| {
                    el.on_click(cx.listener(|this, _, window, cx| {
                        this.navigation.selected = None;
                        this.show_inline(Modal::New, window, cx);
                    }))
                }),
            )
            .child(
                icon_button(
                    "explorer-new-folder",
                    ToolbarIcon::NewFolder,
                    "New folder".into(),
                    enabled,
                )
                .when(enabled, |el| {
                    el.on_click(cx.listener(|this, _, window, cx| {
                        this.navigation.selected = None;
                        this.show_inline(Modal::NewFolder, window, cx);
                    }))
                }),
            )
            .child(
                icon_button(
                    "explorer-sort",
                    ToolbarIcon::Sort,
                    "Change sort order".into(),
                    enabled,
                )
                .when(enabled, |el| {
                    el.on_click(cx.listener(|this, _, window, cx| {
                        let open = this.sort_menu.is_none();
                        this.dismiss(window, cx);
                        if open {
                            this.sort_menu = Some(window.mouse_position() + point(px(0.), px(12.)));
                            this.sort_selected = SortOrder::ALL
                                .iter()
                                .position(|s| *s == this.navigation.sort)
                                .unwrap_or(0);
                            this.focus.focus(window);
                        }
                        cx.notify();
                    }))
                }),
            )
            .child(
                icon_button(
                    "explorer-auto-reveal",
                    ToolbarIcon::AutoReveal,
                    format!(
                        "Auto-reveal current file · {}",
                        if self.navigation.auto_reveal {
                            "On"
                        } else {
                            "Off"
                        }
                    ),
                    enabled,
                )
                .when(enabled, |el| {
                    el.on_click(cx.listener(|this, _, window, cx| {
                        this.dismiss(window, cx);
                        this.navigation.auto_reveal = !this.navigation.auto_reveal;
                        this.auto_revealed = None;
                        this.sync_auto_reveal();
                        cx.notify();
                    }))
                }),
            )
            .child(
                icon_button(
                    "explorer-collapse-all",
                    if expanded {
                        ToolbarIcon::Collapse
                    } else {
                        ToolbarIcon::Expand
                    },
                    if expanded {
                        "Collapse all"
                    } else {
                        "Expand all"
                    }
                    .into(),
                    enabled,
                )
                .when(enabled, |el| {
                    el.on_click(cx.listener(|this, _, window, cx| {
                        this.dismiss(window, cx);
                        this.search.update(cx, |editor, cx| editor.set_text("", cx));
                        if this.navigation.expanded.is_empty() {
                            this.navigation.expanded = this
                                .vault
                                .as_ref()
                                .unwrap()
                                .folders
                                .iter()
                                .cloned()
                                .collect();
                        } else {
                            this.navigation.expanded.clear();
                            this.navigation.selected = None;
                        }
                        this.tree.flatten(&this.navigation.expanded);
                        this.notes_scroll
                            .0
                            .borrow()
                            .base_handle
                            .set_offset(point(px(0.), px(0.)));
                        cx.notify();
                    }))
                }),
            )
    }
    pub(super) fn render_sort_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut content = div()
            .id("sort-menu")
            .debug_selector(|| "sort-menu".into())
            .w(px(258.))
            .p_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(LINE))
            .bg(rgb(PANEL))
            .shadow_lg()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.sort_menu = None;
                cx.notify();
            }));
        for (index, order) in SortOrder::ALL.into_iter().enumerate() {
            content = content.child(
                div()
                    .id(("sort-option", index))
                    .debug_selector(move || format!("sort-option-{index}"))
                    .h(px(32.))
                    .px_3()
                    .rounded_sm()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_sm()
                    .cursor_pointer()
                    .when(index == self.sort_selected, |el| el.bg(rgb(0x343049)))
                    .hover(|s| s.bg(rgb(0x343049)))
                    .child(order.label())
                    .child(if self.navigation.sort == order {
                        "✓"
                    } else {
                        ""
                    })
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.sort_selected != index {
                            this.sort_selected = index;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_sort(order, window, cx);
                        cx.stop_propagation();
                    })),
            );
        }
        deferred(
            anchored()
                .position(self.sort_menu.unwrap())
                .snap_to_window_with_margin(px(8.))
                .child(content),
        )
        .with_priority(2)
        .into_any_element()
    }
    pub(super) fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut sidebar = div()
            .w(px(self.navigation.width))
            .h_full()
            .flex_shrink_0()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(LINE))
            .flex()
            .flex_col()
            .child(self.explorer_toolbar(cx))
            .child(
                div()
                    .mx_3()
                    .h(px(40.))
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(BG))
                    .when(self.vault.is_some(), |field| {
                        field.child(self.search.clone())
                    })
                    .when(self.vault.is_none(), |field| {
                        field.opacity(0.5).child(
                            div()
                                .size_full()
                                .px_3()
                                .flex()
                                .items_center()
                                .text_sm()
                                .text_color(rgb(MUTED))
                                .child("Open a vault to search"),
                        )
                    }),
            )
            .child(
                div()
                    .px_4()
                    .pt_3()
                    .pb_2()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(caption(if self.searching {
                        "SEARCHING…".into()
                    } else if !self.search_query.is_empty() && !self.search_ready() {
                        if self.search_error.is_some() {
                            "RESULTS · search unavailable".into()
                        } else {
                            "RESULTS · indexing…".into()
                        }
                    } else if self.search_query.trim().is_empty() {
                        format!(
                            "NOTES · {}",
                            self.vault.as_ref().map_or(0, |v| v.notes.len())
                        )
                    } else {
                        format!(
                            "RESULTS · {}{}",
                            self.search_results.len(),
                            if self.search_results.len() == opalglass::search::query::RESULT_LIMIT {
                                "+"
                            } else {
                                ""
                            }
                        )
                    })),
            );
        let searching = !self.search_query.trim().is_empty();
        let inline_index = (!searching).then(|| self.inline_insert_index()).flatten();
        let notes = uniform_list(
            if searching { "search-results" } else { "notes" },
            if searching {
                self.search_results.len()
            } else {
                self.tree.visible.len() + usize::from(inline_index.is_some())
            },
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                if searching {
                    this.queue_search_snippets(range.clone(), cx);
                }
                range
                    .filter_map(|index| {
                        if inline_index == Some(index) {
                            return Some(this.inline_row());
                        }
                        let row_index =
                            index - usize::from(inline_index.is_some_and(|i| index > i));
                        let (path, title, folder, depth) = if searching {
                            let row = this.search_results.get(index)?;
                            (row.path.clone(), row.title.clone(), false, 0)
                        } else {
                            let entry = this.tree.visible.get(row_index)?;
                            (entry.path.clone(), entry.name(), entry.folder, entry.depth)
                        };
                        if this.inline_parent.is_some()
                            && this.modal == Modal::Rename
                            && this.modal_target.as_ref() == Some(&path)
                        {
                            return Some(this.inline_row());
                        }
                        let active = this.tabs.get(this.active).is_some_and(|t| t.path == path);
                        let selected = this.navigation.selected.as_ref() == Some(&path);
                        let icon = if folder {
                            if this.navigation.expanded.contains(&path) {
                                "▾ ▱"
                            } else {
                                "▸ ▱"
                            }
                        } else {
                            "  ▤"
                        };
                        let tooltip = this.vault.as_ref()?.root.join(&path).display().to_string();
                        Some(
                            div()
                                .id(("note", index))
                                .debug_selector({
                                    let path = path.clone();
                                    move || format!("entry:{}", path.display())
                                })
                                .h(px(if searching { 98. } else { 32. }))
                                .pl(px(8. + depth as f32 * 16.))
                                .pr_2()
                                .py_1()
                                .rounded_md()
                                .overflow_hidden()
                                .cursor_pointer()
                                .bg(rgb(if active {
                                    0x303047
                                } else if selected {
                                    0x242c3c
                                } else {
                                    PANEL
                                }))
                                .hover(|s| s.bg(rgb(0x292e3e)))
                                .child(
                                    div()
                                        .text_sm()
                                        .truncate()
                                        .text_color(rgb(if active { 0xd5c7ff } else { TEXT }))
                                        .child(format!("{icon}  {title}")),
                                )
                                .when(searching, |el| {
                                    let row = &this.search_results[index];
                                    let excerpt = row.snippet.as_ref().map(|snippet| {
                                        let highlights =
                                            snippet.highlights.iter().cloned().map(|range| {
                                                (
                                                    range,
                                                    HighlightStyle {
                                                        color: Some(rgb(ACCENT).into()),
                                                        font_weight: Some(FontWeight::BOLD),
                                                        ..Default::default()
                                                    },
                                                )
                                            });
                                        let fragment = match snippet.status {
                                            "stale" | "missing" => {
                                                "Note changed; waiting for refresh…".to_owned()
                                            }
                                            "unavailable" | "window_miss" | "positions_limit" => {
                                                "Excerpt unavailable".to_owned()
                                            }
                                            _ => snippet.fragment.replace(['\n', '\r', '\t'], " "),
                                        };
                                        div()
                                            .text_xs()
                                            .text_color(rgb(MUTED))
                                            .h(px(36.))
                                            .overflow_hidden()
                                            .child(
                                                StyledText::new(SharedString::from(fragment))
                                                    .with_highlights(highlights),
                                            )
                                    });
                                    el.child(caption(path.display().to_string()).truncate())
                                        .children(excerpt)
                                        .when(index == this.search_selected, |el| {
                                            el.bg(rgb(0x303047))
                                        })
                                })
                                .tooltip(move |_, cx| {
                                    cx.new(|_| PathTooltip(tooltip.clone())).into()
                                })
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener({
                                        let path = path.clone();
                                        move |this, event: &MouseDownEvent, window, cx| {
                                            this.open_explorer_menu(
                                                Some(path.clone()),
                                                event.position,
                                                window,
                                                cx,
                                            );
                                            cx.stop_propagation();
                                        }
                                    }),
                                )
                                .when(folder, |el| this.folder_drop_target(el, path.clone(), cx))
                                .when(!this.managing, |el| {
                                    el.on_drag(
                                        EntryDrag {
                                            root: this.vault.as_ref().unwrap().root.clone(),
                                            path: path.clone(),
                                        },
                                        |drag, _, _, cx| {
                                            cx.new(|_| PathTooltip(drag.path.display().to_string()))
                                        },
                                    )
                                })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    if this.managing || searching && this.searching {
                                        return;
                                    }
                                    this.dismiss(window, cx);
                                    this.navigation.selected = Some(path.clone());
                                    if folder {
                                        this.tree_focus.focus(window);
                                        this.toggle_folder(path.clone(), cx);
                                    } else {
                                        this.open_note(path.clone(), window, cx);
                                    }
                                }))
                                .into_any_element(),
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(if searching {
            self.results_scroll.clone()
        } else {
            self.notes_scroll.clone()
        })
        .track_focus(&self.tree_focus)
        .flex_1()
        .min_h_0()
        .px_2();
        sidebar =
            sidebar
                .child(
                    div()
                        .id("explorer")
                        .debug_selector(|| "explorer".into())
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                this.open_explorer_menu(None, event.position, window, cx);
                                cx.stop_propagation();
                            }),
                        )
                        .child(notes),
                )
                .child(
                    div()
                        .p_3()
                        .border_t_1()
                        .border_color(rgb(LINE))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(button("open-vault", "Open vault…   Ctrl O").on_click(
                            cx.listener(|this, _, window, cx| this.pick_vault(window, cx)),
                        )),
                );
        sidebar.into_any_element()
    }
}
