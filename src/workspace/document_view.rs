// Active document toolbar, split panes and empty workspace.
use super::*;

impl Workspace {
    pub(super) fn render_document_area(&self, cx: &mut Context<Self>) -> Div {
        let mut main = div().flex_1().min_w_0().h_full().flex().flex_col();
        main = main.child(self.render_tabs(cx));
        if let Some(tab) = self.tabs.get(self.active) {
            let editor = tab.editor.clone();
            let path = tab.path.clone();
            let mut tools = div()
                .px_4()
                .py_2()
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(rgb(LINE))
                .gap_3()
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .child(
                            icon_button(
                                "history-back",
                                ToolbarIcon::Back,
                                "Back · Alt ←".into(),
                                !self.history_back.is_empty(),
                            )
                            .on_click(
                                cx.listener(|this, _, w, cx| this.visit_history(false, w, cx)),
                            ),
                        )
                        .child(
                            icon_button(
                                "history-forward",
                                ToolbarIcon::Forward,
                                "Forward · Alt →".into(),
                                !self.history_forward.is_empty(),
                            )
                            .on_click(
                                cx.listener(|this, _, w, cx| this.visit_history(true, w, cx)),
                            ),
                        ),
                )
                .child(
                    caption(path.display().to_string())
                        .flex_1()
                        .min_w_0()
                        .truncate(),
                );
            let mut modes = div().flex().flex_shrink_0().gap_1().child(
                icon_button(
                    "properties",
                    ToolbarIcon::Sort,
                    "Edit properties".into(),
                    true,
                )
                .on_click(cx.listener(|this, _, window, cx| this.open_properties(window, cx))),
            );
            for (id, label, icon, mode) in [
                ("source", "Source", ToolbarIcon::Source, ViewMode::Source),
                ("split", "Split", ToolbarIcon::Split, ViewMode::Split),
                (
                    "reading",
                    "Reading",
                    ToolbarIcon::Reading,
                    ViewMode::Preview,
                ),
            ] {
                let tooltip = if self.mode == mode {
                    format!("{label} · Current view · Ctrl E to cycle views")
                } else {
                    format!("Switch to {label} · Ctrl E to cycle views")
                };
                modes = modes.child(icon_button(id, icon, tooltip, true).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.set_mode(mode, window, cx);
                    },
                )));
            }
            if self.mode != ViewMode::Preview {
                modes = modes.child(
                    icon_button(
                        "toggle-live-preview",
                        ToolbarIcon::NewNote,
                        if self.live_preview {
                            "Live Preview · Switch to raw Source · Ctrl Shift E"
                        } else {
                            "Raw Source · Enable Live Preview · Ctrl Shift E"
                        }
                        .into(),
                        true,
                    )
                    .on_click(
                        cx.listener(|this, _, window, cx| this.toggle_live_preview(window, cx)),
                    ),
                );
            }
            modes = modes.child(
                icon_button(
                    "toggle-outline",
                    ToolbarIcon::Outline,
                    "Heading outline · Ctrl Shift O".into(),
                    true,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.outline_open = !this.outline_open;
                    cx.notify();
                })),
            );
            tools = tools.child(
                modes
                    .child(div().w(px(8.)))
                    .child(
                        icon_button("save", ToolbarIcon::Save, "Save · Ctrl S".into(), true)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.save_tab(this.active, cx);
                            })),
                    )
                    .child(
                        icon_button(
                            "copy",
                            ToolbarIcon::Copy,
                            "Save copy · Ctrl Shift S".into(),
                            true,
                        )
                        .on_click(
                            cx.listener(|this, _, w, cx| this.show_modal(Modal::Copy, w, cx)),
                        ),
                    )
                    .child(
                        icon_button(
                            "reload",
                            ToolbarIcon::Reload,
                            "Reload from disk".into(),
                            true,
                        )
                        .on_click(
                            cx.listener(|this, _, w, cx| this.show_modal(Modal::Reload, w, cx)),
                        ),
                    ),
            );
            main = main.child(tools);
            if tab.external_changed {
                main = main.child(
                    div()
                        .px_4()
                        .py_2()
                        .bg(rgb(0x302b25))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(caption(
                            "This note changed outside Opalglass. Your edits are preserved.",
                        ))
                        .child(
                            button("compare-external", "Compare versions")
                                .debug_selector(|| "compare-external".into())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_comparison(window, cx)
                                })),
                        ),
                );
            }
            if self.find.open {
                main = main.child(self.find_bar(cx));
            }
            let mut panes = div()
                .id("attachment-drop-zone")
                .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(rgb(0x28283b)))
                .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                    if let Some(tab) = this.tabs.get(this.active) {
                        tab.editor.update(cx, |editor, cx| {
                            editor.place_drop_caret(window.mouse_position(), cx)
                        });
                        this.import_attachments(
                            tab.editor.clone(),
                            paths
                                .paths()
                                .iter()
                                .cloned()
                                .map(opalglass::attachments::Input::File)
                                .collect(),
                            window,
                            cx,
                        );
                    }
                }))
                .debug_selector(|| "note-panes".into())
                .flex_1()
                .min_h_0()
                .flex();
            if self.mode != ViewMode::Preview {
                panes = panes.child(
                    div()
                        .debug_selector(|| "source-pane".into())
                        .flex_1()
                        .when(self.mode == ViewMode::Split, |mut pane| {
                            pane.style().flex_grow = Some(self.split_ratio);
                            pane
                        })
                        .min_w_0()
                        .h_full()
                        .child(
                            AnyView::from(editor).cached(StyleRefinement::default().size_full()),
                        ),
                );
            }
            if self.mode == ViewMode::Split {
                let bounds = self.split_bounds.clone();
                panes = panes
                    .relative()
                    .child(
                        canvas(move |rect, _, _| bounds.set(rect), |_, _, _, _| ())
                            .absolute()
                            .size_full(),
                    )
                    .child(
                        div()
                            .id("split-resize")
                            .debug_selector(|| "split-resize".into())
                            .w(px(SPLIT_DIVIDER_WIDTH))
                            .h_full()
                            .flex_shrink_0()
                            .cursor_col_resize()
                            .hover(|s| s.bg(rgb(0x303749)))
                            .flex()
                            .justify_center()
                            .child(div().w(px(1.)).h_full().bg(rgb(LINE)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                    let bounds = this.split_bounds.get();
                                    let available =
                                        f32::from(bounds.size.width) - SPLIT_DIVIDER_WIDTH;
                                    let center = f32::from(bounds.left())
                                        + available * this.split_ratio
                                        + SPLIT_DIVIDER_WIDTH / 2.;
                                    this.split_drag_offset =
                                        Some(f32::from(event.position.x) - center);
                                    window.prevent_default();
                                    cx.stop_propagation();
                                }),
                            ),
                    );
            }
            if self.mode != ViewMode::Source {
                panes = panes.child(
                    div()
                        .debug_selector(|| "preview-pane".into())
                        .flex_1()
                        .when(self.mode == ViewMode::Split, |mut pane| {
                            pane.style().flex_grow = Some(1. - self.split_ratio);
                            pane
                        })
                        .min_w_0()
                        .h_full()
                        .child(
                            AnyView::from(self.preview_pane.clone())
                                .cached(StyleRefinement::default().size_full()),
                        )
                        .relative()
                        .when(self.mode == ViewMode::Split, |pane| {
                            pane.child(
                                div().absolute().top(px(2.)).left(px(4.)).child(
                                    icon_button(
                                        "lock-reading-scroll",
                                        if self.scroll_locked {
                                            ToolbarIcon::Locked
                                        } else {
                                            ToolbarIcon::Unlocked
                                        },
                                        if self.scroll_locked {
                                            "Unlock reading scroll"
                                        } else {
                                            "Lock reading scroll to editor"
                                        }
                                        .into(),
                                        true,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.scroll_locked = !this.scroll_locked;
                                            if !this.scroll_locked {
                                                this.preview_pane
                                                    .update(cx, |pane, _| pane.pending_sync = None);
                                            }
                                            cx.notify();
                                        },
                                    )),
                                ),
                            )
                        }),
                );
            }
            if tab.initial_frame_pending {
                let owner = cx.entity().downgrade();
                let editor_id = tab.editor.entity_id();
                panes = panes.relative().child(
                    canvas(
                        |_, _, _| (),
                        move |_, _, window, cx| {
                            // Schedule from paint, not from the load callback: even a
                            // single worker must draw the opening view before bulk work.
                            window.defer(cx, move |_, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    if let Some(tab) = this
                                        .tabs
                                        .iter_mut()
                                        .find(|tab| tab.editor.entity_id() == editor_id)
                                        && tab.initial_frame_pending
                                    {
                                        tab.initial_frame_pending = false;
                                        this.update_document(cx);
                                        // The file watcher may have populated contents
                                        // while this window was waiting to be painted.
                                        // Search still needs its own index in that case.
                                        if this.index_revision != this.revision {
                                            cx.notify();
                                        }
                                    }
                                });
                            });
                        },
                    )
                    .absolute()
                    .size_full(),
                );
            }
            main = main.child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(panes)
                    .when(self.outline_open, |row| row.child(self.outline(cx))),
            );
            let rows = self.connections.read(cx).items.len().div_ceil(3).min(3);
            let height = 34. + rows as f32 * 30.;
            main = main.child(
                div().h(px(height)).flex_shrink_0().child(
                    AnyView::from(self.connections.clone())
                        .cached(StyleRefinement::default().size_full()),
                ),
            );
        } else {
            main =
                main.child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .items_center()
                        .gap_4()
                        .child(div().text_size(px(52.)).text_color(rgb(ACCENT)).child("◈"))
                        .child(
                            div()
                                .text_3xl()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("A little space to think."),
                        )
                        .child(
                            div()
                                .text_color(rgb(MUTED))
                                .child("Plain Markdown. Connected ideas. Entirely yours."),
                        )
                        .child(button("welcome-open", "Open a vault folder").on_click(
                            cx.listener(|this, _, window, cx| this.pick_vault(window, cx)),
                        ))
                        .child(caption(
                            "Ctrl N  new note     ·     Ctrl P  find a note     ·     Ctrl S  save",
                        )),
                );
        }
        main
    }
}
