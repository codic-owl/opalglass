// Editor preferences, settings controls and note outline.
use super::*;

impl Workspace {
    pub(super) fn apply_preferences(&mut self, cx: &mut Context<Self>) {
        for tab in &self.tabs {
            tab.editor.update(cx, |e, cx| {
                e.set_preferences(
                    self.preferences.font_size,
                    self.preferences.line_numbers,
                    cx,
                )
            });
        }
        self.preview_pane.update(cx, |_, cx| cx.notify());
        if let Some(directory) = &self.state_directory
            && let Err(error) = self.preferences.save(directory)
        {
            self.message(format!("Could not save settings: {error}"), true, cx);
        }
        cx.notify();
    }
    pub(super) fn settings_panel(&self, cx: &mut Context<Self>) -> AnyElement {
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
                    .w(px(460.))
                    .p_6()
                    .rounded_lg()
                    .bg(rgb(PANEL))
                    .border_1()
                    .border_color(rgb(LINE))
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(div().text_xl().child("Settings"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child("Text size")
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        button("font-smaller", "−")
                                            .debug_selector(|| "font-smaller".into())
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.preferences.font_size =
                                                    (this.preferences.font_size - 1.).max(10.);
                                                this.apply_preferences(cx);
                                            })),
                                    )
                                    .child(format!("{} px", self.preferences.font_size as u32))
                                    .child(
                                        button("font-larger", "+")
                                            .debug_selector(|| "font-larger".into())
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.preferences.font_size =
                                                    (this.preferences.font_size + 1.).min(28.);
                                                this.apply_preferences(cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        button(
                            "setting-line-numbers",
                            if self.preferences.line_numbers {
                                "Line numbers: On"
                            } else {
                                "Line numbers: Off"
                            },
                        )
                        .debug_selector(|| "setting-line-numbers".into())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.preferences.line_numbers = !this.preferences.line_numbers;
                            this.apply_preferences(cx);
                        })),
                    )
                    .child(
                        button(
                            "setting-default-editor",
                            if self.preferences.default_live_preview {
                                "Default editing mode: Live Preview"
                            } else {
                                "Default editing mode: Source"
                            },
                        )
                        .debug_selector(|| "setting-default-editor".into())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.preferences.default_live_preview =
                                !this.preferences.default_live_preview;
                            this.apply_preferences(cx);
                        })),
                    )
                    .child(caption(
                        "Default editing mode applies when opening a workspace.",
                    ))
                    .child(
                        button(
                            "setting-autosave",
                            if self.preferences.autosave {
                                "Autosave: On"
                            } else {
                                "Autosave: Off"
                            },
                        )
                        .debug_selector(|| "setting-autosave".into())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.preferences.autosave = !this.preferences.autosave;
                            this.apply_preferences(cx);
                        })),
                    )
                    .child(
                        button("settings-close", "Done")
                            .debug_selector(|| "settings-close".into())
                            .on_click(cx.listener(|this, _, w, cx| {
                                this.settings_open = false;
                                this.focus_document(w, cx);
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }
    pub(super) fn outline(&self, cx: &mut Context<Self>) -> AnyElement {
        let count = self
            .tabs
            .get(self.active)
            .map_or(0, |t| t.prepared.headings.len());
        div()
            .debug_selector(|| "heading-outline".into())
            .w(px(220.))
            .h_full()
            .flex_shrink_0()
            .border_l_1()
            .border_color(rgb(LINE))
            .flex()
            .flex_col()
            .child(div().p_3().child(caption("OUTLINE")))
            .when(count == 0, |el| {
                el.child(div().p_3().child(caption("No headings in this note")))
            })
            .child(
                uniform_list(
                    "outline-list",
                    count,
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|index| {
                                let tab = &this.tabs[this.active];
                                let (title, row) = &tab.prepared.headings[index];
                                let level =
                                    if let Kind::Heading(level) = tab.prepared.rows[*row].kind {
                                        level
                                    } else {
                                        1
                                    };
                                div()
                                    .id(("outline-heading", index))
                                    .debug_selector(move || format!("outline-heading-{index}"))
                                    .h(px(32.))
                                    .pl(px(12. + (level - 1) as f32 * 12.))
                                    .pr_2()
                                    .text_sm()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(0x303749)))
                                    .child(div().truncate().child(title.clone()))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let Some(tab) = this.tabs.get_mut(this.active) else {
                                            return;
                                        };
                                        let row = tab.prepared.headings[index].1;
                                        let byte = tab.prepared.rows[row].source.start;
                                        tab.editor.update(cx, |e, cx| e.jump_to(byte, cx));
                                        tab.reveal_preview_row(row);
                                        this.preview_pane.update(cx, |_, cx| cx.notify());
                                        this.focus_document(window, cx);
                                    }))
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(self.outline_scroll.clone())
                .flex_1()
                .min_h_0(),
            )
            .into_any_element()
    }
}
