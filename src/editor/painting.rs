// Plain input painting for single-line fields and fallback rendering.
use super::*;

impl Editor {
    pub(super) fn paint(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.bounds = bounds;
        let text = self.buffer.text();
        let selected = self.buffer.selection();
        let mut cuts = vec![0, selected.start, selected.end, text.len()];
        if let Some(range) = &self.buffer.marked {
            cuts.extend([range.start, range.end]);
        }
        cuts.sort_unstable();
        cuts.dedup();
        let runs: Vec<_> = cuts
            .windows(2)
            .filter(|pair| pair[0] < pair[1])
            .map(|pair| TextRun {
                len: pair[1] - pair[0],
                font: font(MONO_FONT),
                color: rgb(0xd6d9e3).into(),
                background_color: (pair[0] >= selected.start && pair[1] <= selected.end)
                    .then(|| rgb(0x38496a).into()),
                underline: self
                    .buffer
                    .marked
                    .as_ref()
                    .filter(|r| pair[0] >= r.start && pair[1] <= r.end)
                    .map(|_| UnderlineStyle {
                        thickness: px(1.),
                        color: Some(rgb(0xb6a1ff).into()),
                        wavy: false,
                    }),
                strikethrough: None,
            })
            .collect();
        let width =
            (bounds.size.width - px(if self.single_line { 24. } else { PAD * 2. })).max(px(20.));
        let shaped = window
            .text_system()
            .shape_text(
                text.to_owned().into(),
                px(14.),
                &runs,
                (!self.single_line).then_some(width),
                None,
            )
            .unwrap_or_default();
        let mut start = 0;
        let mut top = px(0.);
        self.rows = shaped
            .into_iter()
            .map(|line| {
                let row = Row { start, top, line };
                start += row.line.len() + 1;
                top += row.line.size(px(LINE)).height;
                row
            })
            .collect();
        self.content_height = top + px(PAD * 2.);
        if self.reveal {
            let caret = self.point_for(self.buffer.cursor()) - bounds.origin;
            if self.single_line {
                if caret.x < px(12.) {
                    self.scroll_x = (self.scroll_x + caret.x - px(12.)).max(px(0.));
                } else if caret.x + px(14.) > bounds.size.width {
                    self.scroll_x += caret.x + px(14.) - bounds.size.width;
                }
            } else if caret.y < px(8.) {
                self.scroll = (self.scroll + caret.y - px(8.)).max(px(0.));
            } else if caret.y + px(LINE + 8.) > bounds.size.height {
                self.scroll += caret.y + px(LINE + 8.) - bounds.size.height;
            }
            self.reveal = false;
        }
        self.scroll = self.scroll.clamp(
            px(0.),
            (self.content_height - bounds.size.height).max(px(0.)),
        );
        let origin = self.origin();
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if self.buffer.text().is_empty() && !self.placeholder.is_empty() {
                let run = TextRun {
                    len: self.placeholder.len(),
                    font: font(MONO_FONT),
                    color: rgb(0x687187).into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let line = window.text_system().shape_line(
                    self.placeholder.clone().into(),
                    px(14.),
                    &[run],
                    None,
                );
                let _ = line.paint(origin, px(LINE), window, cx);
            }
            for row in &self.rows {
                let pos = origin + point(px(0.), row.top);
                if pos.y + row.line.size(px(LINE)).height < bounds.origin.y
                    || pos.y > bounds.bottom()
                {
                    continue;
                }
                let _ = row
                    .line
                    .paint_background(pos, px(LINE), TextAlign::Left, None, window, cx);
                let _ = row
                    .line
                    .paint(pos, px(LINE), TextAlign::Left, None, window, cx);
            }
            if self.focus.is_focused(window) {
                let pos = self.point_for(self.buffer.cursor());
                window.paint_quad(fill(
                    Bounds::new(pos + point(px(0.), px(3.)), size(px(2.), px(LINE - 5.))),
                    rgb(0xb7a5ff),
                ));
            }
            if self.content_height > bounds.size.height && !self.single_line {
                let height =
                    (bounds.size.height * (bounds.size.height / self.content_height)).max(px(24.));
                let y = (bounds.size.height - height)
                    * (self.scroll / (self.content_height - bounds.size.height));
                window.paint_quad(fill(
                    Bounds::new(
                        point(bounds.right() - px(5.), bounds.top() + y),
                        size(px(3.), height),
                    ),
                    rgb(0x414b61),
                ));
            }
        });
        window.handle_input(
            &self.focus,
            ElementInputHandler::new(bounds, cx.entity()),
            cx,
        );
    }
}
