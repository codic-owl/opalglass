//! Source line-number layout, kept separate from text shaping.
use super::*;

const NUMBER_GAP: f32 = 24.;
// The fixed 12 px mono number font fits in an 8 px cell on supported platforms.
const NUMBER_CELL: f32 = 8.;
const MIN_GUTTER: f32 = 64.;

impl Editor {
    pub(super) fn gutter(&self) -> f32 {
        if !self.line_numbers {
            return PAD;
        }
        // Buffer uses its line-start index; sizing never scans the note body.
        let digits = self.buffer.line_number(self.buffer.text().len()).ilog10() + 1;
        MIN_GUTTER.max(PAD + digits as f32 * NUMBER_CELL + NUMBER_GAP)
    }

    pub(super) fn paint_line_number(
        &self,
        start: usize,
        bounds: Bounds<Pixels>,
        line_height: Pixels,
        active: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        if !self.line_numbers || self.buffer.line_start(start) != start {
            return None;
        }
        let number = self.buffer.line_number(start).to_string();
        let number_line = window.text_system().shape_line(
            number.clone().into(),
            px(12.),
            &[TextRun {
                len: number.len(),
                font: font(MONO_FONT),
                color: rgb(if active { 0xb8a5fa } else { 0x7d879c }).into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            None,
        );
        let number_origin = point(
            bounds.left() - self.callout_indent(start) + px(self.gutter() - NUMBER_GAP)
                - number_line.width,
            bounds.top(),
        );
        let _ = number_line.paint(number_origin, line_height, window, cx);
        Some(Bounds::new(
            number_origin,
            size(number_line.width, line_height),
        ))
    }
}
