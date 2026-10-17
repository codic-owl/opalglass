// Virtualized Reading view and native Markdown elements.
use super::*;

impl Workspace {
    pub(super) fn preview(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.rebuild_reading_composition(cx);
        let tab = &self.tabs[self.active];
        let context = reading_context::ReadingContext::host(tab);
        let prepared = context.prepared.clone();
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
            cx.processor(move |this, list_index, _, cx| this.composed_preview_row(list_index, cx)),
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
        context: &reading_context::ReadingContext,
        index: usize,
        row: &PreviewRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut content = self.preview_content(context, index, row, cx);
        let ancestors: Vec<_> = context
            .prepared
            .callouts
            .iter()
            .filter(|c| {
                c.rows.contains(&index)
                    && context
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
            let projection = &context.projection;
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
        self.preview_footnote(context, index, content, cx)
    }
    pub(super) fn preview_content(
        &self,
        context: &reading_context::ReadingContext,
        index: usize,
        row: &PreviewRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(images) = self.preview_images(context, index, row, cx) {
            return images;
        }

        let mut style = match row.kind {
            Kind::CalloutTitle => return self.preview_callout(context, index, row, cx),
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
            Kind::Code => return self.preview_code(context, index, row, cx),
            Kind::Rule => return div().h(px(1.)).my_3().bg(rgb(LINE)).into_any_element(),
            Kind::Table => return self.preview_table(context, index, row, cx),
        };
        if !row.continuation {
            style = style.mt(px(match row.kind {
                Kind::Heading(_) if index > 0 => 28.,
                Kind::Heading(_) => 0.,
                Kind::Item | Kind::Task(_) => 6.,
                _ => 16.,
            }));
        }
        let text = self.preview_text(
            context,
            row.text.clone(),
            &row.spans,
            context.id("inline", index),
            cx,
        );
        if matches!(row.kind, Kind::Task(_)) && !row.continuation && row.task.is_some() {
            return self.preview_task(context, index, row, style, text, cx);
        }
        style.child(text).into_any_element()
    }
}
