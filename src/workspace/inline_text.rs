// Shared native inline typography and link interaction.
use super::*;

impl Workspace {
    pub(super) fn preview_fragment(
        &self,
        context: &reading_context::ReadingContext,
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
            context,
            row.text[range].into(),
            &spans,
            context.id(format!("fragment-{index}"), part),
            cx,
        )
    }
    pub(super) fn preview_text(
        &self,
        context: &reading_context::ReadingContext,
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
                    .map(|target| self.link_highlight_from(&context.path, target));
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
        let has_references = spans
            .iter()
            .any(|span| crate::theme::typography::is_reference(&span.style));
        let highlights = highlights.into_iter().map(|(range, mut style)| {
            if spans.iter().any(|span| {
                span.range.contains(&range.start)
                    && crate::theme::typography::is_reference(&span.style)
            }) {
                style.fade_out = Some(1.);
                style.underline = None;
            }
            (range, style)
        });
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
        let context = context.clone();
        let styled = StyledText::new(SharedString::from(text.clone())).with_highlights(highlights);
        let layout = styled.layout().clone();
        let interactive =
            InteractiveText::new(id, styled).on_click(ranges, move |index, window, cx| {
                if let Some(target) = targets.get(index) {
                    if external_link(target) {
                        cx.open_url(target);
                    } else {
                        let _ = entity.update(cx, |this, cx| {
                            this.follow_reading(context.clone(), target.clone(), window, cx)
                        });
                    }
                }
            });
        if has_references {
            div()
                .relative()
                .child(interactive)
                .child(crate::theme::typography::reference_overlay(
                    text,
                    spans.to_vec(),
                    layout,
                ))
                .into_any_element()
        } else {
            interactive.into_any_element()
        }
    }
}
