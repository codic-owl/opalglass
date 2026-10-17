// Workspace layout and top-level rendering.
use super::*;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(debug_assertions)]
        self.performance.update(cx, |bar, _| bar.mark_frame_start());
        self.update_picker(cx);
        self.update_search(window, cx);
        self.update_find(cx);
        self.update_completion(window, cx);
        self.update_tree();
        if self.modal == Modal::Move {
            self.update_move_results(cx);
        }
        if !self.managing {
            self.update_document(cx);
        }
        let main = self.render_document_area(cx);
        let metrics = self
            .tabs
            .get(self.active)
            .map(|tab| {
                let buffer = &tab.editor.read(cx).buffer;
                let (line, column) = buffer.line_column();
                if tab.prepared.complete {
                    format!(
                        "{} words   ·   Ln {line}, Col {column}   ·   UTF-8",
                        tab.prepared.words
                    )
                } else {
                    format!("Ln {line}, Col {column}   ·   UTF-8")
                }
            })
            .unwrap_or_default();
        let mut root = div()
            .size_full()
            .relative()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .font_family(UI_FONT)
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.resizing_sidebar && event.pressed_button == Some(MouseButton::Left) {
                    this.navigation.width =
                        (f32::from(event.position.x) - SIDEBAR_RAIL_WIDTH).clamp(180., 600.);
                    cx.notify();
                } else if let Some(offset) = this.split_drag_offset
                    && event.pressed_button == Some(MouseButton::Left)
                {
                    let bounds = this.split_bounds.get();
                    let available = f32::from(bounds.size.width) - SPLIT_DIVIDER_WIDTH;
                    if available > 0. {
                        // Keep both panes usable, including in narrow windows.
                        let minimum = (160. / available).min(0.5);
                        this.split_ratio = ((f32::from(event.position.x - bounds.left())
                            - offset
                            - SPLIT_DIVIDER_WIDTH / 2.)
                            / available)
                            .clamp(minimum, 1. - minimum);
                        cx.notify();
                    }
                } else {
                    this.resizing_sidebar = false;
                    this.split_drag_offset = None;
                }
            }))
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _, w, cx| this.visit_history(false, w, cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|this, _, w, cx| this.visit_history(true, w, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.resizing_sidebar = false;
                    this.split_drag_offset = None;
                }),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .debug_selector(|| "sidebar-rail".into())
                            .w(px(SIDEBAR_RAIL_WIDTH))
                            .h_full()
                            .flex_shrink_0()
                            .bg(rgb(0x181c25))
                            .border_r_1()
                            .border_color(rgb(LINE))
                            .flex()
                            .flex_col()
                            .items_center()
                            .pt(px(7.))
                            .child(
                                icon_button(
                                    "toggle-sidebar",
                                    ToolbarIcon::Panel,
                                    if self.navigation.hidden {
                                        "Expand sidebar · Ctrl B"
                                    } else {
                                        "Collapse sidebar · Ctrl B"
                                    }
                                    .into(),
                                    true,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.navigation.hidden = !this.navigation.hidden;
                                        this.sort_menu = None;
                                        this.focus_document(window, cx);
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                icon_button(
                                    "command-palette",
                                    ToolbarIcon::Commands,
                                    "Command palette · Ctrl Shift P".into(),
                                    true,
                                )
                                .on_click(cx.listener(
                                    |this, _, w, cx| this.show_picker(PickerMode::Commands, w, cx),
                                )),
                            )
                            .child(
                                icon_button(
                                    "bookmarks",
                                    ToolbarIcon::Bookmark,
                                    "Bookmarks".into(),
                                    self.vault.is_some(),
                                )
                                .on_click(cx.listener(
                                    |this, _, w, cx| this.show_picker(PickerMode::Bookmarks, w, cx),
                                )),
                            )
                            .child(
                                icon_button(
                                    "daily-note",
                                    ToolbarIcon::Calendar,
                                    "Open today's note · Ctrl Alt D".into(),
                                    self.vault.is_some(),
                                )
                                .on_click(
                                    cx.listener(|this, _, w, cx| this.open_daily_note(w, cx)),
                                ),
                            )
                            .child(div().flex_1())
                            .child(
                                icon_button(
                                    "settings",
                                    ToolbarIcon::Settings,
                                    "Settings · Ctrl ,".into(),
                                    true,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.settings_open = true;
                                        this.focus.focus(window);
                                        cx.notify();
                                    },
                                )),
                            )
                            .pb_2(),
                    )
                    .when(!self.navigation.hidden, |el| {
                        el.child(
                            div()
                                .w(px(self.navigation.width))
                                .h_full()
                                .flex_shrink_0()
                                .child(
                                    AnyView::from(self.sidebar.clone())
                                        .cached(StyleRefinement::default().size_full()),
                                ),
                        )
                        .child(
                            div()
                                .id("sidebar-resize")
                                .debug_selector(|| "sidebar-resize".into())
                                .w(px(5.))
                                .h_full()
                                .flex_shrink_0()
                                .cursor_col_resize()
                                .hover(|s| s.bg(rgb(ACCENT)))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| {
                                        this.resizing_sidebar = true;
                                        cx.stop_propagation();
                                    }),
                                ),
                        )
                    })
                    .child(main),
            )
            .child(
                div()
                    .px_4()
                    .py_2()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(rgb(LINE))
                    .flex()
                    .gap_3()
                    .justify_between()
                    .text_xs()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(rgb(if self.error { 0xf1b18e } else { MUTED }))
                            .child(self.status.clone()),
                    )
                    .map(|footer| {
                        #[cfg(debug_assertions)]
                        let footer = footer.child(
                            div().w(px(230.)).h(px(18.)).flex_shrink_0().child(
                                AnyView::from(self.performance.clone())
                                    .cached(StyleRefinement::default().size_full()),
                            ),
                        );
                        footer
                    })
                    .when(self.indexing, |footer| {
                        footer.child(
                            div().w(px(180.)).h(px(18.)).flex_shrink_0().child(
                                AnyView::from(self.indexing_bar.clone())
                                    .cached(StyleRefinement::default().size_full()),
                            ),
                        )
                    })
                    .child(caption(metrics)),
            );
        if !self.completion.candidates.is_empty() {
            root = root.child(self.completion_menu(window, cx));
        }
        if self.picker.is_some() {
            root = root.child(self.picker_panel(cx));
        }
        if self.replace_panel.is_some() {
            root = root.child(self.render_replace(cx));
        }
        if self.mentions.is_some() {
            root = root.child(self.render_mentions(cx));
        }
        if self.history_panel.is_some() {
            root = root.child(self.render_history(cx));
        }
        if self.comparison.is_some() {
            root = root.child(self.render_comparison(cx));
        }
        if self.properties_panel.is_some() {
            root = root.child(self.render_properties(cx));
        }
        if self.settings_open {
            root = root.child(self.settings_panel(cx));
        }
        if self.sort_menu.is_some() && !self.managing {
            root = root.child(self.render_sort_menu(cx));
        }
        if self.explorer_menu.is_some() && !self.managing {
            root = root.child(self.render_explorer_menu(cx));
        }
        if self.modal == Modal::Move && !self.managing {
            root = root.child(self.render_move_picker(cx));
        }
        if !matches!(self.modal, Modal::None | Modal::Move)
            && self.inline_parent.is_none()
            && !self.managing
        {
            let (title, hint, action) = match self.modal {
                Modal::New => (
                    "New note",
                    "A name or folder/name. The .md extension is added for you.",
                    "Create note",
                ),
                Modal::Copy => (
                    "Save a copy",
                    "Preserve your current edits in a separate Markdown file.",
                    "Save copy",
                ),
                Modal::OpenPath => (
                    "Open vault by path",
                    "Enter the full path to an existing folder of Markdown notes.",
                    "Open folder",
                ),
                Modal::Reload => (
                    "Reload from disk?",
                    "This discards unsaved edits in the current tab. Save a copy first to keep them.",
                    "Discard edits and reload",
                ),
                Modal::NewFolder => (
                    "New folder",
                    "Enter a folder path. Its parent must already exist.",
                    "Create folder",
                ),
                Modal::Rename => (
                    "Rename",
                    "A new name. Saves open edits and updates resolved wiki links.",
                    "Rename",
                ),
                Modal::Delete => (
                    "Move to trash?",
                    "Unsaved edits are saved first. Restore this item from the explorer’s right-click menu.",
                    "Move to trash",
                ),
                Modal::Move | Modal::None | Modal::RestoreTrash => unreachable!(),
            };
            root = root.child(
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
                            .w(px(560.))
                            .p_6()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(0x49435f))
                            .bg(rgb(PANEL))
                            .shadow_lg()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .child(caption(hint))
                            .when(self.modal == Modal::Delete, |el| {
                                el.child(
                                    div().text_sm().child(
                                        self.modal_target
                                            .as_ref()
                                            .map(|p| p.display().to_string())
                                            .unwrap_or_default(),
                                    ),
                                )
                            })
                            .when(!matches!(self.modal, Modal::Reload | Modal::Delete), |el| {
                                el.child(
                                    div()
                                        .h(px(44.))
                                        .border_1()
                                        .border_color(rgb(ACCENT))
                                        .rounded_md()
                                        .bg(rgb(BG))
                                        .child(self.input.clone()),
                                )
                            })
                            .when(self.error, |el| {
                                el.child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(0xf1b18e))
                                        .child(self.status.clone()),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .gap_3()
                                    .child(button("cancel", "Cancel").on_click(
                                        cx.listener(|this, _, w, cx| this.dismiss(w, cx)),
                                    ))
                                    .child(button("confirm", action).on_click(
                                        cx.listener(|this, _, w, cx| this.submit(w, cx)),
                                    )),
                            ),
                    ),
            );
        }
        if self.managing {
            root = root.child(
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
                            .p_6()
                            .rounded_lg()
                            .bg(rgb(PANEL))
                            .child(self.status.clone()),
                    ),
            );
        }
        root
    }
}
