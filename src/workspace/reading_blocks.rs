//! Interactive Reading blocks retain their document and instance identity.
use super::*;

impl Workspace {
    pub(super) fn preview_callout(
        &self,
        context: &reading_context::ReadingContext,
        index: usize,
        row: &PreviewRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let callouts = &context.prepared.callouts;
        let Some(callout) = callouts
            .binary_search_by_key(&index, |c| c.rows.start)
            .ok()
            .map(|at| &callouts[at])
        else {
            return self.preview_text(
                context,
                row.text.clone(),
                &row.spans,
                context.id("callout-title", index),
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
                context,
                row.text.clone(),
                &row.spans,
                context.id("callout-title", index),
                cx,
            )));
        if callout.collapsed.is_some() {
            let collapsed = context.projection.collapsed(callout);
            let id = callout.source.start;
            let selector = context.selector(format!("fold-callout-{index}"));
            let button_id = context.id("fold-callout", index);
            let context = context.clone();
            header = header.child(
                button(button_id, if collapsed { "›" } else { "⌄" })
                    .debug_selector(move || selector.clone())
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
                        this.toggle_reading_callout(context.clone(), id, collapsed, cx)
                    })),
            );
        }
        header.into_any_element()
    }

    pub(super) fn preview_code(
        &self,
        context: &reading_context::ReadingContext,
        index: usize,
        row: &PreviewRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let code = row.code.clone().unwrap_or_else(|| row.text.clone());
        let complete = context.prepared.complete;
        let selector = context.selector(format!("copy-code-{index}"));
        div()
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
                            button(context.id("copy-code", index), "Copy code")
                                .debug_selector(move || selector.clone())
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
                        context,
                        row.text.clone(),
                        &row.spans,
                        context.id("code-text", index),
                        cx,
                    )),
            )
            .into_any_element()
    }

    pub(super) fn preview_task(
        &self,
        context: &reading_context::ReadingContext,
        index: usize,
        row: &PreviewRow,
        style: Div,
        text: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(range) = row.task.clone() else {
            return style.child(text).into_any_element();
        };
        let done = matches!(row.kind, Kind::Task(true));
        let selector = context.selector(format!("reading-task-{index}"));
        let id = context.id("reading-task", index);
        let context = context.clone();
        style
            .flex()
            .items_start()
            .gap_2()
            .child(
                button(id, if done { "☑" } else { "☐" })
                    .debug_selector(move || selector.clone())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.toggle_reading_task(context.clone(), range.clone(), done, window, cx);
                    })),
            )
            .child(div().flex_1().min_w_0().child(text))
            .into_any_element()
    }
}
