// Each virtualized item shapes a bounded slice of every cell. Continuations keep
// the row's shared column widths, background and outer borders.
use super::*;

impl Workspace {
    pub(super) fn preview_table(
        &self,
        context: &reading_context::ReadingContext,
        index: usize,
        row: &PreviewRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(chunk) = &row.table else {
            return div().into_any_element();
        };
        let table = &chunk.row;
        let mut cells = div()
            .flex()
            .w_full()
            .min_w(px(table.cells.len() as f32 * 100.))
            .border_l_1()
            .when(chunk.first, |el| el.border_t_1())
            .border_color(rgb(LINE));
        for (column, cell) in table.cells.iter().enumerate() {
            let text = self.preview_text(
                context,
                chunk.text(column).into(),
                &chunk.spans(column),
                context.id(format!("table-{index}"), column),
                cx,
            );
            let selector = context.selector(format!("reading-table-{index}-{column}"));
            cells = cells.child(
                div()
                    .debug_selector(move || selector.clone())
                    .flex_1()
                    .min_w_0()
                    .px_3()
                    .when(chunk.first, |el| el.pt_2())
                    .when(chunk.last, |el| el.pb_2().border_b_1())
                    .border_r_1()
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
        div()
            .id(context.id("reading-table-row", index))
            .overflow_x_scroll()
            .when(table.header && chunk.first, |el| el.mt_4())
            .child(cells)
            .into_any_element()
    }
}
