//! Footnote presentation and links are scoped to their original document.
use super::*;

impl Workspace {
    pub(super) fn preview_footnote(
        &self,
        context: &reading_context::ReadingContext,
        index: usize,
        content: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(definition) = context.prepared.footnotes.at_row(index) {
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
            let projection = &context.projection;
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
                                let id = context.id(format!("footnote-return-{index}"), occurrence);
                                let selector = context
                                    .selector(format!("footnote-return-{index}-{occurrence}"));
                                let context = context.clone();
                                button(
                                    id,
                                    if occurrence == 0 {
                                        "↩︎".into()
                                    } else {
                                        format!("↩︎{}", occurrence + 1)
                                    },
                                )
                                .debug_selector(move || selector.clone())
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
                                        this.follow_reading(
                                            context.clone(),
                                            target.clone(),
                                            window,
                                            cx,
                                        )
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
}
