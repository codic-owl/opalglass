// Native text layout, painting and mouse interactions.
use super::*;

impl Editor {
    pub(super) fn source_row(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let range = self.source_ranges[index].clone();
        let selection = self.buffer.selection();
        let selection = selection.start.clamp(range.start, range.end)
            ..selection.end.clamp(range.start, range.end);
        let marked =
            self.buffer.marked.as_ref().map(|r| {
                r.start.clamp(range.start, range.end)..r.end.clamp(range.start, range.end)
            });
        let revealed = !self.live_preview
            || self
                .live_revealed
                .as_ref()
                .is_some_and(|r| r.start <= range.start && range.start < r.end);
        let width = self.source_width;
        let cached = self
            .cached_rows
            .iter()
            .find(|r| {
                r.range == range
                    && r.width == width
                    && r.selection == selection
                    && r.marked == marked
                    && r.revealed == revealed
            })
            .cloned();
        let shaped = if let Some(cached) = cached {
            cached
        } else {
            let projection = self.projection(&range, revealed);
            let to_display = |source: usize| {
                projection.display_index(source.saturating_sub(range.start).min(range.len()))
            };
            let display_selection = to_display(selection.start)..to_display(selection.end);
            let display_marked = marked
                .as_ref()
                .map(|r| to_display(r.start)..to_display(r.end));
            let mut cuts = vec![
                0,
                projection.text.len(),
                display_selection.start,
                display_selection.end,
            ];
            let first_match = self.find_matches.partition_point(|r| r.end <= range.start);
            let matches: Vec<_> = self.find_matches[first_match..]
                .iter()
                .take_while(|r| r.start < range.end)
                .collect();
            for r in &matches {
                cuts.extend([to_display(r.start), to_display(r.end)]);
            }
            let first_link = self.source_links.partition_point(|r| r.end <= range.start);
            let links: Vec<_> = self.source_links[first_link..]
                .iter()
                .take_while(|r| r.start < range.end)
                .collect();
            let first_missing = self.missing_links.partition_point(|r| r.end <= range.start);
            let missing: Vec<_> = self.missing_links[first_missing..]
                .iter()
                .take_while(|r| r.start < range.end)
                .collect();
            for r in &links {
                cuts.extend([to_display(r.start), to_display(r.end)]);
            }
            for span in &projection.spans {
                cuts.extend([span.range.start, span.range.end]);
            }
            let first_syntax = self
                .source_syntax
                .partition_point(|s| s.range.end <= range.start);
            let syntax: Vec<_> = self.source_syntax[first_syntax..]
                .iter()
                .take_while(|s| s.range.start < range.end)
                .collect();
            for span in &syntax {
                cuts.extend([to_display(span.range.start), to_display(span.range.end)]);
            }
            if let Some(r) = &display_marked {
                cuts.extend([r.start, r.end]);
            }
            cuts.sort_unstable();
            cuts.dedup();
            let heading = if let live::Kind::Heading(level) = projection.kind {
                Some(level)
            } else {
                None
            };
            let code = matches!(projection.kind, live::Kind::Code | live::Kind::Fence);
            let font_size = if let Some(level) = heading {
                match level {
                    1 => 32.,
                    2 => 25.,
                    3 => 21.,
                    _ => 18.,
                }
            } else if self.live_preview && !code {
                16.
            } else {
                14.
            };
            let scale = self.font_size / 14.;
            let line_height = px(if self.live_preview {
                (font_size * 1.5_f32).max(LINE)
            } else {
                LINE
            }) * scale;
            let font_size = font_size * scale;
            let prefix = self
                .buffer
                .text()
                .as_bytes()
                .get(self.buffer.line_start(range.start));
            let runs: Vec<_> = cuts
                .windows(2)
                .filter(|p| p[0] < p[1])
                .map(|pair| {
                    let source = range.start + projection.source_index(pair[0]);
                    let style = projection
                        .spans
                        .iter()
                        .find(|s| s.range.contains(&pair[0]))
                        .map(|s| &s.style);
                    let is_missing = missing.iter().any(|r| r.contains(&source));
                    let is_link = links.iter().any(|r| r.contains(&source))
                        || style.is_some_and(|s| s.target.is_some());
                    let inline_code = style.is_some_and(|s| s.code);
                    let token = syntax
                        .iter()
                        .find(|s| s.range.contains(&source))
                        .map(|s| s.style);
                    let mut text_font = font(if self.live_preview && !code && !inline_code {
                        UI_FONT
                    } else {
                        MONO_FONT
                    });
                    if heading.is_some()
                        || style.is_some_and(|s| s.bold)
                        || token.is_some_and(|s| s.bold)
                    {
                        text_font.weight = FontWeight::BOLD;
                    }
                    if style.is_some_and(|s| s.italic) || token.is_some_and(|s| s.italic) {
                        text_font.style = FontStyle::Italic;
                    }
                    TextRun {
                        len: pair[1] - pair[0],
                        font: text_font,
                        color: rgb(if is_missing {
                            MISSING_LINK
                        } else if is_link || (!self.live_preview && prefix == Some(&b'#')) {
                            0xb8a5fa
                        } else if heading.is_some() {
                            0xf0edf9
                        } else if let Some(token) = token {
                            token.color
                        } else if code || inline_code {
                            0xc2d8ce
                        } else if projection.kind == live::Kind::Quote
                            || (!self.live_preview && prefix == Some(&b'>'))
                        {
                            0x9bbfb3
                        } else {
                            0xd6d9e3
                        })
                        .into(),
                        background_color: if pair[0] >= display_selection.start
                            && pair[1] <= display_selection.end
                        {
                            Some(rgb(0x38496a).into())
                        } else if matches.iter().any(|r| r.contains(&source)) {
                            Some(rgb(0x655224).into())
                        } else {
                            inline_code.then(|| rgb(0x252e37).into()).or_else(|| {
                                style
                                    .and_then(|s| s.highlight)
                                    .map(|color| rgb(crate::theme::highlight_color(color)).into())
                            })
                        },
                        underline: if display_marked
                            .as_ref()
                            .is_some_and(|r| r.start <= pair[0] && pair[1] <= r.end)
                        {
                            Some(UnderlineStyle {
                                thickness: px(1.),
                                color: Some(rgb(0xb6a1ff).into()),
                                wavy: false,
                            })
                        } else if is_missing {
                            Some(missing_link_underline())
                        } else {
                            is_link.then_some(UnderlineStyle {
                                thickness: px(1.),
                                color: None,
                                wavy: false,
                            })
                        },
                        strikethrough: style.is_some_and(|s| s.strike).then_some(
                            StrikethroughStyle {
                                thickness: px(1.),
                                color: None,
                            },
                        ),
                    }
                })
                .collect();
            let line = window
                .text_system()
                .shape_text(
                    projection.text.clone().into(),
                    px(font_size),
                    &runs,
                    Some(width),
                    None,
                )
                .unwrap_or_default()
                .into_iter()
                .next()
                .unwrap_or_default();
            let shaped = CachedRow {
                range: range.clone(),
                width,
                selection,
                marked,
                line,
                projection,
                line_height,
                revealed,
            };
            if self.cached_rows.len() >= 256 {
                self.cached_rows.drain(..128);
            }
            self.cached_rows.push(shaped.clone());
            shaped
        };
        let CachedRow {
            line,
            projection,
            line_height,
            ..
        } = shaped;
        let height = line.size(line_height).height.max(line_height);
        let entity = cx.entity();
        canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                entity.update(cx, |editor, cx| {
                    #[cfg(test)]
                    editor
                        .first_paint_at
                        .get_or_insert_with(std::time::Instant::now);
                    #[cfg(test)]
                    editor
                        .first_paint_indexed_rows
                        .get_or_insert(editor.source_ranges.len());
                    let gutter = editor.gutter();
                    let origin = bounds.origin + point(px(gutter), px(0.));
                    let active = editor.buffer.line_start(editor.buffer.cursor())
                        == editor.buffer.line_start(range.start);
                    if matches!(projection.kind, live::Kind::Code | live::Kind::Fence) {
                        window.paint_quad(fill(
                            Bounds::new(
                                origin - point(px(8.), px(0.)),
                                size(bounds.size.width - px(gutter - 8.), bounds.size.height),
                            ),
                            rgb(0x10131a),
                        ));
                    } else if active
                        && editor.focus.is_focused(window)
                        && editor.buffer.selection().is_empty()
                    {
                        window.paint_quad(fill(bounds, rgb(0x1d2230)));
                    }
                    if projection.kind == live::Kind::Quote {
                        window.paint_quad(fill(
                            Bounds::new(origin - point(px(9.), px(0.)), size(px(2.), height)),
                            rgb(0xb8a5fa),
                        ));
                    }
                    if projection.kind == live::Kind::Rule && projection.text.is_empty() {
                        window.paint_quad(fill(
                            Bounds::new(
                                origin + point(px(0.), height / 2.),
                                size(editor.source_width, px(1.)),
                            ),
                            rgb(0x3b4254),
                        ));
                    }
                    if editor.line_numbers && editor.buffer.line_start(range.start) == range.start {
                        let number = editor.buffer.line_number(range.start).to_string();
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
                        let number_origin =
                            bounds.origin + point(px(gutter - 16.) - number_line.width, px(0.));
                        let _ = number_line.paint(number_origin, line_height, window, cx);
                    }
                    editor.visible_rows.push(SourceRow {
                        range: range.clone(),
                        origin,
                        line: line.clone(),
                        projection: projection.clone(),
                        line_height,
                    });
                    let _ = line.paint_background(
                        origin,
                        line_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                    let _ = line.paint(origin, line_height, TextAlign::Left, None, window, cx);
                    if editor.buffer.text().is_empty() && !editor.placeholder.is_empty() {
                        let placeholder = window.text_system().shape_line(
                            editor.placeholder.clone().into(),
                            px(14.),
                            &[TextRun {
                                len: editor.placeholder.len(),
                                font: font(MONO_FONT),
                                color: rgb(0x687187).into(),
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            }],
                            None,
                        );
                        let _ = placeholder.paint(origin, line_height, window, cx);
                    }
                    let cursor = editor.buffer.cursor();
                    let cursor_row = editor
                        .source_ranges
                        .partition_point(|r| r.start <= cursor)
                        .saturating_sub(1);
                    if editor.focus.is_focused(window) && cursor_row == index {
                        let pos = origin
                            + line
                                .position_for_index(
                                    projection.display_index(cursor - range.start),
                                    line_height,
                                )
                                .unwrap_or_default();
                        window.paint_quad(fill(
                            Bounds::new(
                                pos + point(px(0.), px(3.)),
                                size(px(2.), line_height - px(5.)),
                            ),
                            rgb(0xb7a5ff),
                        ));
                    }
                });
            },
        )
        .w_full()
        .h(height)
        .into_any_element()
    }
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

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.render_count += 1;
        }
        self.prepare_source();
        let entity = cx.entity();
        let content = if self.single_line {
            canvas(
                |_, _, _| (),
                move |bounds, _, window, cx| {
                    entity.update(cx, |editor, cx| editor.paint(bounds, window, cx));
                },
            )
            .size_full()
            .into_any_element()
        } else {
            let prepaint_entity = entity.clone();
            let input_entity = entity.clone();
            div()
                .size_full()
                .relative()
                .child(
                    canvas(
                        move |bounds, window, cx| {
                            prepaint_entity.update(cx, |editor, _| {
                                editor.bounds = bounds;
                                editor.visible_rows.clear();
                                let width = (bounds.size.width - px(PAD * 2. + editor.gutter()))
                                    .max(px(20.));
                                if editor.source_width != width {
                                    let had_width = editor.source_width > px(0.);
                                    editor.source_width = width;
                                    editor.cached_rows.clear();
                                    // The initial list is already unmeasured. Rebuilding
                                    // every item again here doubles first-frame work.
                                    if had_width {
                                        let offset = editor.source_list.logical_scroll_top();
                                        editor.source_list.splice(
                                            0..editor.source_list.item_count(),
                                            editor.source_ranges.len(),
                                        );
                                        editor.source_list.scroll_to(offset);
                                    }
                                }
                                let _ = window;
                            });
                        },
                        move |bounds, _, window, cx| {
                            input_entity.update(cx, |editor, cx| {
                                editor.publish_scroll_anchor(cx);
                                window.handle_input(
                                    &editor.focus,
                                    ElementInputHandler::new(bounds, cx.entity()),
                                    cx,
                                );
                                if !editor.source_complete
                                    && editor.source_requested_revision != editor.source_version
                                {
                                    let revision = editor.source_version;
                                    editor.source_requested_revision = revision;
                                    let owner = cx.entity().downgrade();
                                    window.defer(cx, move |_, cx| {
                                        let _ = owner.update(cx, |editor, cx| {
                                            editor.start_source_index(revision, cx);
                                        });
                                    });
                                }
                            });
                        },
                    )
                    .absolute()
                    .size_full(),
                )
                .child(
                    list(
                        self.source_list.clone(),
                        cx.processor(|this, index, window, cx| this.source_row(index, window, cx)),
                    )
                    .size_full()
                    .px(px(PAD))
                    .py(px(PAD)),
                )
                .into_any_element()
        };
        div()
            .size_full()
            .overflow_hidden()
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .on_key_down(cx.listener(Self::key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.focus.focus(window);
                    this.preferred_x = None;
                    let index = this.index_for(event.position);
                    if !this.single_line
                        && (event.modifiers.control || event.modifiers.platform)
                        && !this.code_blocks.iter().any(|range| range.contains(&index))
                        && !opalglass::properties::frontmatter(this.buffer.text())
                            .ok()
                            .flatten()
                            .is_some_and(|h| h.range.contains(&index))
                    {
                        let start = this.buffer.line_start(index);
                        let end = this.buffer.line_end(index);
                        let (_, spans, mapping) =
                            opalglass::markdown::inline_mapped(&this.buffer.text()[start..end]);
                        if let Some(target) = spans.iter().find_map(|span| {
                            let range =
                                start + mapping[span.range.start]..start + mapping[span.range.end];
                            range
                                .contains(&index)
                                .then(|| span.style.target.clone())
                                .flatten()
                        }) {
                            cx.emit(FollowLink(target));
                            cx.stop_propagation();
                            return;
                        }
                    }
                    this.buffer.move_to(index, event.modifiers.shift);
                    if event.click_count >= 3 {
                        this.buffer
                            .select(this.buffer.line_start(index), this.buffer.line_end(index));
                    } else if event.click_count == 2 {
                        let text = this.buffer.text();
                        let start = text[..index]
                            .char_indices()
                            .rev()
                            .find(|(_, c)| c.is_whitespace())
                            .map_or(0, |(i, c)| i + c.len_utf8());
                        let end = text[index..]
                            .find(char::is_whitespace)
                            .map_or(text.len(), |i| index + i);
                        this.buffer.select(start, end);
                    }
                    this.dragging = true;
                    cx.stop_propagation();
                    this.changed(cx);
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.dragging && event.pressed_button == Some(MouseButton::Left) {
                    this.buffer.move_to(this.index_for(event.position), true);
                    this.changed(cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = false),
            )
            .child(content)
    }
}
