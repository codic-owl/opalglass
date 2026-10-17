//! Callout decoration stays outside the text canvas and its source hit testing.
use super::*;

const INSET: f32 = 16.;
const TITLE_ICON: f32 = 24.;

#[derive(Clone, Copy)]
struct Band {
    depth: usize,
    color: u32,
}

impl Editor {
    fn live_callout(&self, byte: usize) -> Option<(&live::Context, usize)> {
        if !self.live_preview {
            return None;
        }
        let (_, context) = self
            .live_context
            .as_ref()
            .filter(|(revision, _)| *revision == self.buffer.revision())?;
        Some((context, context.callout_at(byte)?))
    }

    pub(super) fn callout_indent(&self, byte: usize) -> Pixels {
        self.live_callout(byte).map_or(px(0.), |(context, index)| {
            let callout = &context.callouts()[index];
            px(INSET * callout.depth.clamp(1, 8) as f32
                + if callout.title.contains(&byte) {
                    TITLE_ICON
                } else {
                    0.
                })
        })
    }

    pub(super) fn callout_color(&self, byte: usize) -> Option<u32> {
        let (context, index) = self.live_callout(byte)?;
        let callout = &context.callouts()[index];
        callout
            .title
            .contains(&byte)
            .then(|| callout.callout.color())
    }

    pub(super) fn source_view(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let byte = self.source_ranges[index].start;
        let Some((context, inner)) = self.live_callout(byte) else {
            return self.source_row(index, window, cx);
        };
        let current = &context.callouts()[inner];
        let title = byte == current.title.start;
        let id = current.callout.source.start;
        let symbol = current.callout.symbol();
        let color = current.callout.color();
        let depth = current.depth.clamp(1, 8);
        let foldable = current.callout.collapsed.is_some();
        let collapsed = self.collapsed(id);
        let indent = self.callout_indent(byte);
        let mut bands = Vec::new();
        let mut ancestor = Some(inner);
        while let Some(index) = ancestor {
            let callout = &context.callouts()[index];
            bands.push(Band {
                depth: callout.depth.clamp(1, 8),
                color: callout.callout.color(),
            });
            ancestor = callout.parent;
            if bands.len() == 8 {
                break;
            }
        }
        bands.reverse();
        let gutter = self.gutter();
        let row = self.source_row(index, window, cx);
        let mut content = div()
            .relative()
            .w_full()
            .pl(indent)
            .child(background(bands.clone(), gutter))
            .child(row)
            .child(foreground(
                bands,
                gutter,
                title.then_some((depth, symbol, color)),
            ));
        if title && foldable {
            let label = if collapsed {
                "Expand callout"
            } else {
                "Collapse callout"
            };
            content = content.child(
                div()
                    .id(("live-callout-toggle", id))
                    .debug_selector(move || format!("live-callout-toggle-{id}"))
                    .absolute()
                    .left(px(gutter + INSET * (depth - 1) as f32 - 20.))
                    .top_0()
                    .w(px(18.))
                    .h(px(LINE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .text_color(rgb(color))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(0x303749)))
                    .tooltip(move |_, cx| cx.new(|_| CalloutTooltip(label)).into())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |editor, _, _, cx| {
                        editor.toggle_callout(id, cx);
                        cx.stop_propagation();
                    }))
                    .child(if collapsed { "▸" } else { "▾" }),
            );
        }
        content.into_any_element()
    }
}

fn background(bands: Vec<Band>, gutter: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            for band in &bands {
                let left = px(gutter + INSET * (band.depth - 1) as f32);
                let width = (bounds.size.width - left).max(px(0.));
                window.paint_quad(fill(
                    Bounds::new(
                        bounds.origin + point(left, px(0.)),
                        size(width, bounds.size.height),
                    ),
                    rgba((band.color << 8) | 20),
                ));
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

fn foreground(
    bands: Vec<Band>,
    gutter: f32,
    title: Option<(usize, &'static str, u32)>,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| {
            for band in &bands {
                window.paint_quad(fill(
                    Bounds::new(
                        bounds.origin + point(px(gutter + INSET * (band.depth - 1) as f32), px(0.)),
                        size(px(2.), bounds.size.height),
                    ),
                    rgb(band.color),
                ));
            }
            if let Some((depth, symbol, color)) = title {
                let text: SharedString = symbol.into();
                let line = window.text_system().shape_line(
                    text.clone(),
                    px(16.),
                    &[TextRun {
                        len: text.len(),
                        font: font(UI_FONT),
                        color: rgb(color).into(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    None,
                );
                let origin = bounds.origin + point(px(gutter + INSET * depth as f32), px(0.));
                let _ = line.paint(origin, px(LINE), window, cx);
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

struct CalloutTooltip(&'static str);

impl Render for CalloutTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(0x10131a))
            .text_color(rgb(0xd6d9e3))
            .text_sm()
            .child(self.0)
    }
}
