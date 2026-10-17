use super::*;

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
                                            editor.visibility.rows.len(),
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
                        cx.processor(|this, index, window, cx| {
                            let physical = this.visibility.rows.row(index);
                            this.source_view(physical, window, cx)
                        }),
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
                        && let Some(target) = this.link_target_at(index)
                    {
                        cx.emit(FollowLink(target));
                        cx.stop_propagation();
                        return;
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
