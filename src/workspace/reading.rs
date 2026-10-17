// Virtualized Reading view and native Markdown elements.
use super::*;

impl Workspace {
    pub(super) fn preview(&self, cx: &mut Context<Self>) -> AnyElement {
        let tab = &self.tabs[self.active];
        let prepared = tab.prepared.clone();
        let projection = tab.preview_projection.clone();
        if prepared.complete && prepared.rows.is_empty() {
            return div()
                .flex_1()
                .h_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .child(
                    div()
                        .text_lg()
                        .text_color(rgb(TEXT))
                        .child("Your note starts here"),
                )
                .child(caption("Switch to Source to start writing."))
                .child(
                    button("empty-source", "Start writing")
                        .debug_selector(|| "empty-source".into())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.set_mode(ViewMode::Source, window, cx)
                        })),
                )
                .into_any_element();
        }
        let rows = list(
            tab.preview_scroll.clone(),
            cx.processor(move |this, list_index, _, cx| {
                let index = projection.row(list_index);
                div()
                    .w_full()
                    // GPUI List measures rows at its full width and only uses
                    // vertical padding. Put horizontal gutters on each row so
                    // they survive narrow panes and keep the margins scrollable.
                    .px_6()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(760.))
                            .debug_selector(move || format!("preview-column-{index}"))
                            .min_w_0()
                            .child(this.preview_row(index, &prepared.rows[index], cx)),
                    )
                    .into_any_element()
            }),
        )
        .flex_1()
        .min_w_0()
        .min_h_0()
        .h_full()
        .py_8();
        // List paints its rows, not a Div background. The surrounding surface
        // supplies typography and color consistently in Reading and Split.
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(rgb(0x181c25))
            .text_size(px(16. * self.preferences.font_size / 14.))
            .line_height(relative(1.7))
            .child(rows)
            .into_any_element()
    }
    pub(super) fn preview_row(
        &self,
        index: usize,
        row: &PreviewRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut content = self.preview_content(index, row, cx);
        let ancestors: Vec<_> = self.tabs[self.active]
            .prepared
            .callouts
            .iter()
            .filter(|c| {
                c.rows.contains(&index)
                    && self.tabs[self.active]
                        .prepared
                        .footnotes
                        .at_row(index)
                        .is_none_or(|d| d.sections.iter().any(|r| r.contains(&c.rows.start)))
            })
            .collect();
        for _ in 0..row.quotes.saturating_sub(ancestors.len()).min(8) {
            content = div()
                .border_l_2()
                .border_color(rgb(ACCENT))
                .pl_4()
                .bg(rgb(0x202532))
                .child(content)
                .into_any_element();
        }
        for callout in ancestors.iter().rev().take(8) {
            let projection = &self.tabs[self.active].preview_projection;
            let last = projection.row(projection.list_index(callout.rows.end.saturating_sub(1)));
            content = div()
                .flex()
                .flex_col()
                .border_l_2()
                .border_color(rgb(callout.color()))
                .px_4()
                .bg(rgba((callout.color() << 8) | 20))
                .when(index == callout.rows.start, |el| el.mt_4().rounded_t_md())
                .when(index == last, |el| el.pb_3().rounded_b_md())
                .child(content)
                .into_any_element();
        }
        let content = div()
            .pl(px(row.indent.min(8) as f32 * 24.))
            .child(content)
            .into_any_element();
        if let Some(definition) = self.tabs[self.active].prepared.footnotes.at_row(index) {
            let first = index == definition.rows.start;
            let mut block = div()
                .flex()
                .flex_col()
                .text_size(px(14. * self.preferences.font_size / 14.))
                .when(first && definition.number == 1, |el| {
                    el.mt_6().border_t_1().border_color(rgb(LINE)).pt_3()
                })
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap_2()
                        .child(div().w(px(24.)).flex_shrink_0().pt_4().child(if first {
                            format!("{}.", definition.number)
                        } else {
                            String::new()
                        }))
                        .child(div().flex_1().min_w_0().child(content)),
                );
            let projection = &self.tabs[self.active].preview_projection;
            if definition.sections.last().is_some_and(|r| {
                projection.row(projection.list_index(r.end.saturating_sub(1))) == index
            }) {
                block = block.child(
                    div().pl_8().flex().gap_2().children(
                        definition
                            .references
                            .iter()
                            .enumerate()
                            .map(|(occurrence, at)| {
                                let target = format!("{}{at}", opalglass::footnotes::BACK_PREFIX);
                                button(
                                    (
                                        SharedString::from(format!("footnote-return-{index}")),
                                        occurrence,
                                    ),
                                    if occurrence == 0 {
                                        "↩︎".into()
                                    } else {
                                        format!("↩︎{}", occurrence + 1)
                                    },
                                )
                                .debug_selector(move || {
                                    format!("footnote-return-{index}-{occurrence}")
                                })
                                .tooltip(move |_, cx| {
                                    cx.new(|_| {
                                        PathTooltip(format!(
                                            "Return to reference {}",
                                            occurrence + 1
                                        ))
                                    })
                                    .into()
                                })
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.follow(target.clone(), window, cx)
                                    },
                                ))
                            }),
                    ),
                );
            }
            return block.into_any_element();
        }
        content
    }
    pub(super) fn preview_content(
        &self,
        index: usize,
        row: &PreviewRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if row.spans.iter().any(|span| span.style.image)
            && let (Some(vault), Some(tab)) = (&self.vault, self.tabs.get(self.active))
        {
            let mut content = div().my_3().flex().flex_col().gap_2();
            let mut start = 0;
            for (part, span) in row
                .spans
                .iter()
                .enumerate()
                .filter(|(_, span)| span.style.image)
            {
                let Some(target) = span.style.target.as_deref() else {
                    continue;
                };
                let path = opalglass::attachments::resolve(&vault.root, &tab.path, target)
                    .ok()
                    .or_else(|| {
                        vault
                            .resolve_attachment(&tab.path, target)
                            .and_then(|p| vault.file_path(&p).ok())
                    })
                    .filter(|p| opalglass::attachments::is_image(p));
                let Some(path) = path else {
                    continue;
                };
                if start < span.range.start {
                    content = content.child(self.preview_fragment(
                        row,
                        start..span.range.start,
                        index,
                        part * 2,
                        cx,
                    ));
                }
                content = content.child(
                    div()
                        .id((SharedString::from(format!("attachment-{index}")), part))
                        .debug_selector(move || format!("attachment-image-{index}"))
                        .child(
                            img(path.clone())
                                .w_full()
                                .h(px(320.))
                                .object_fit(ObjectFit::Contain),
                        )
                        .cursor_pointer()
                        .on_click(move |_, _, cx| cx.open_with_system(&path)),
                );
                start = span.range.end;
            }
            if start > 0 {
                if start < row.text.len() {
                    content = content.child(self.preview_fragment(
                        row,
                        start..row.text.len(),
                        index,
                        usize::MAX,
                        cx,
                    ));
                }
                return content.into_any_element();
            }
        }

        let mut style = match row.kind {
            Kind::CalloutTitle => {
                let tab = &self.tabs[self.active];
                let Some(callout) = tab.prepared.callouts.iter().find(|c| c.rows.start == index)
                else {
                    return self.preview_text(
                        row.text.clone(),
                        &row.spans,
                        ("callout-title", index).into(),
                        cx,
                    );
                };
                let mut header = div()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_color(rgb(callout.color()))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(callout.symbol())
                    .child(div().flex_1().min_w_0().child(self.preview_text(
                        row.text.clone(),
                        &row.spans,
                        ("callout-title", index).into(),
                        cx,
                    )));
                if callout.collapsed.is_some() {
                    let collapsed = tab.preview_projection.collapsed(callout);
                    let path = tab.path.clone();
                    let revision = tab.prepared_revision;
                    let id = callout.source.start;
                    header = header.child(
                        button(("fold-callout", index), if collapsed { "›" } else { "⌄" })
                            .debug_selector(move || format!("fold-callout-{index}"))
                            .tooltip(move |_, cx| {
                                cx.new(|_| {
                                    PathTooltip(
                                        if collapsed {
                                            "Expand callout"
                                        } else {
                                            "Collapse callout"
                                        }
                                        .into(),
                                    )
                                })
                                .into()
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let Some(tab) = this.tabs.get_mut(this.active) else {
                                    return;
                                };
                                if tab.path != path || tab.prepared_revision != revision {
                                    return;
                                }
                                let offset = tab.preview_scroll.logical_scroll_top();
                                let row = tab.preview_projection.row(offset.item_ix);
                                tab.preview_projection.overrides.insert(id, !collapsed);
                                tab.preview_projection
                                    .rebuild(tab.prepared.rows.len(), &tab.prepared.callouts);
                                tab.preview_projection.with_footnotes(
                                    tab.prepared.rows.len(),
                                    &tab.prepared.footnotes,
                                    &tab.prepared.callouts,
                                );
                                tab.preview_scroll.splice(
                                    0..tab.preview_scroll.item_count(),
                                    tab.preview_projection.len(tab.prepared.rows.len()),
                                );
                                tab.preview_scroll.scroll_to(ListOffset {
                                    item_ix: tab.preview_projection.list_index(row),
                                    offset_in_item: offset.offset_in_item,
                                });
                                this.preview_pane.update(cx, |_, cx| cx.notify());
                                cx.notify();
                            })),
                    );
                }
                return header.into_any_element();
            }
            Kind::Heading(level) => div()
                .line_height(relative(1.3))
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(match level {
                    1 => 32.,
                    2 => 25.,
                    3 => 21.,
                    _ => 18.,
                } * self.preferences.font_size
                    / 14.))
                .text_color(rgb(0xf0edf9)),
            Kind::Paragraph => div(),
            Kind::Quote => div().py_2().text_color(rgb(0xb7c1d4)),
            Kind::Item => div().pl_2(),
            Kind::Task(done) => div()
                .pl_2()
                .text_color(rgb(if done { MUTED } else { TEXT })),
            Kind::Code => {
                let code = row.code.clone().unwrap_or_else(|| row.text.clone());
                let complete = self.tabs[self.active].prepared.complete;
                return div()
                    .when(!row.continuation, |el| {
                        el.mt_4().child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .px_4()
                                .py_1()
                                .border_b_1()
                                .border_color(rgb(LINE))
                                .child(caption(
                                    row.language.as_deref().unwrap_or("CODE").to_string(),
                                ))
                                .child(
                                    button(("copy-code", index), "Copy code")
                                        .debug_selector(move || format!("copy-code-{index}"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if !complete {
                                                this.message(
                                                    "This code block is still loading.",
                                                    false,
                                                    cx,
                                                );
                                                return;
                                            }
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                code.to_string(),
                                            ));
                                            this.message("Code copied to clipboard", false, cx);
                                        })),
                                ),
                        )
                    })
                    .bg(rgb(0x10131a))
                    .child(
                        div()
                            .px_4()
                            .py_2()
                            .font_family(MONO_FONT)
                            .text_size(px(self.preferences.font_size))
                            .line_height(relative(1.6))
                            .text_color(rgb(0xc2d8ce))
                            .child(self.preview_text(
                                row.text.clone(),
                                &row.spans,
                                ("code-text", index).into(),
                                cx,
                            )),
                    )
                    .into_any_element();
            }
            Kind::Rule => return div().h(px(1.)).my_3().bg(rgb(LINE)).into_any_element(),
            Kind::Table => {
                let Some(table) = &row.table else {
                    return div().into_any_element();
                };
                let mut cells = div()
                    .flex()
                    .w_full()
                    .min_w(px(table.cells.len() as f32 * 100.))
                    .border_l_1()
                    .border_t_1()
                    .border_color(rgb(LINE));
                for (column, cell) in table.cells.iter().enumerate() {
                    let text = self.preview_text(
                        cell.text.clone(),
                        &cell.spans,
                        (SharedString::from(format!("table-{index}")), column).into(),
                        cx,
                    );
                    cells = cells.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px_3()
                            .py_2()
                            .border_r_1()
                            .border_b_1()
                            .border_color(rgb(LINE))
                            .when(table.header, |el| {
                                el.font_weight(FontWeight::SEMIBOLD).bg(rgb(0x242936))
                            })
                            .when(
                                cell.alignment == opalglass::rendering::Alignment::Center,
                                |el| el.text_center(),
                            )
                            .when(
                                cell.alignment == opalglass::rendering::Alignment::Right,
                                |el| el.text_right(),
                            )
                            .child(text),
                    );
                }
                return div()
                    .id(("reading-table-row", index))
                    .overflow_x_scroll()
                    .when(table.header, |el| el.mt_4())
                    .child(cells)
                    .into_any_element();
            }
        };
        if !row.continuation {
            style = style.mt(px(match row.kind {
                Kind::Heading(_) if index > 0 => 28.,
                Kind::Heading(_) => 0.,
                Kind::Item | Kind::Task(_) => 6.,
                _ => 16.,
            }));
        }
        let text = self.preview_text(row.text.clone(), &row.spans, ("inline", index).into(), cx);
        if let Kind::Task(done) = row.kind
            && !row.continuation
            && let Some(range) = row.task.clone()
            && let Some(tab) = self.tabs.get(self.active)
        {
            let path = tab.path.clone();
            let revision = tab.prepared_revision;
            return style.flex().items_start().gap_2().child(
                button(("reading-task", index), if done { "☑" } else { "☐" })
                    .debug_selector(move || format!("reading-task-{index}"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let Some(tab) = this.tabs.get(this.active) else { return; };
                        if tab.path != path || tab.editor.read(cx).buffer.revision() != revision {
                            this.message("The note changed. Try the checkbox again when the preview updates.", false, cx);
                            return;
                        }
                        tab.editor.update(cx, |editor, cx| {
                            let marker = editor.buffer.text().get(range.clone());
                            if marker.is_some_and(|m| opalglass::editing::task_marker(m).is_some()) {
                                editor.buffer.replace(range.clone(), if done { "[ ]" } else { "[x]" });
                                cx.notify();
                            }
                        });
                        cx.notify();
                    }))
            ).child(div().flex_1().min_w_0().child(text)).into_any_element();
        }
        style.child(text).into_any_element()
    }
    pub(super) fn preview_fragment(
        &self,
        row: &PreviewRow,
        range: std::ops::Range<usize>,
        index: usize,
        part: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let spans: Vec<_> = row
            .spans
            .iter()
            .filter(|s| s.range.start < range.end && s.range.end > range.start)
            .map(|s| markdown::Span {
                range: s.range.start.max(range.start) - range.start
                    ..s.range.end.min(range.end) - range.start,
                style: s.style.clone(),
            })
            .collect();
        self.preview_text(
            row.text[range].into(),
            &spans,
            (SharedString::from(format!("fragment-{index}")), part).into(),
            cx,
        )
    }
    pub(super) fn preview_text(
        &self,
        text: Arc<str>,
        spans: &[markdown::Span],
        id: ElementId,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let highlights = spans
            .iter()
            .map(|span| {
                let s = &span.style;
                let link = s
                    .target
                    .as_deref()
                    .map(|target| self.link_highlight(target));
                (
                    span.range.clone(),
                    HighlightStyle {
                        color: if let Some(link) = &link {
                            link.color
                        } else if s.code {
                            Some(rgb(0xa9d5c3).into())
                        } else {
                            s.syntax_color.map(|color| rgb(color).into())
                        },
                        underline: link.and_then(|style| style.underline),
                        font_weight: s.bold.then_some(FontWeight::BOLD),
                        font_style: s.italic.then_some(FontStyle::Italic),
                        background_color: s.code.then(|| rgb(0x252e37).into()).or_else(|| {
                            s.highlight
                                .map(|color| rgb(crate::theme::highlight_color(color)).into())
                        }),
                        strikethrough: s.strike.then_some(StrikethroughStyle {
                            thickness: px(1.),
                            color: None,
                        }),
                        ..Default::default()
                    },
                )
            })
            .collect::<Vec<_>>();
        let highlights = if self.find.open {
            let matches = opalglass::editing::find(
                &text,
                self.find.input.read(cx).buffer.text(),
                self.find.case_sensitive,
            );
            let mut cuts = vec![0, text.len()];
            for (r, _) in &highlights {
                cuts.extend([r.start, r.end]);
            }
            for r in &matches {
                cuts.extend([r.start, r.end]);
            }
            cuts.sort_unstable();
            cuts.dedup();
            cuts.windows(2)
                .filter(|p| p[0] < p[1])
                .map(|p| {
                    let mut style = highlights
                        .iter()
                        .find(|(r, _)| r.contains(&p[0]))
                        .map_or(HighlightStyle::default(), |(_, s)| *s);
                    if matches.iter().any(|r| r.contains(&p[0])) {
                        style.background_color = Some(rgb(0x655224).into());
                    }
                    (p[0]..p[1], style)
                })
                .collect()
        } else {
            highlights
        };
        let ranges = spans
            .iter()
            .filter(|s| s.style.target.is_some())
            .map(|s| s.range.clone())
            .collect();
        let targets = spans
            .iter()
            .filter_map(|s| s.style.target.clone())
            .collect::<Vec<_>>();
        let entity = cx.entity().downgrade();
        let styled = StyledText::new(SharedString::from(text.clone())).with_highlights(highlights);
        let interactive =
            InteractiveText::new(id, styled).on_click(ranges, move |index, window, cx| {
                if let Some(target) = targets.get(index) {
                    if external_link(target) {
                        cx.open_url(target);
                    } else {
                        let _ =
                            entity.update(cx, |this, cx| this.follow(target.clone(), window, cx));
                    }
                }
            });
        interactive.into_any_element()
    }
}
