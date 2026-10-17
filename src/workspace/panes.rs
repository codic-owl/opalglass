// Cached preview, connections and sidebar entities.
use super::*;

pub(super) struct PathTooltip(pub(super) String);
impl Render for PathTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .p_2()
            .rounded_md()
            .bg(rgb(BG))
            .border_1()
            .border_color(rgb(LINE))
            .text_color(rgb(TEXT))
            .text_sm()
            .child(self.0.clone())
    }
}

// Separate cached views keep scrolling confined to the pane that changed.
pub(super) struct PreviewPane {
    pub(super) pending_sync: Option<(ListState, usize, f32)>,
    #[cfg(test)]
    pub(super) render_count: usize,
    pub(super) owner: WeakEntity<Workspace>,
}
impl Render for PreviewPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.render_count += 1;
        }
        let content = self
            .owner
            .update(cx, |workspace, cx| workspace.preview(cx))
            .unwrap_or_else(|_| div().into_any_element());
        let pending = self.pending_sync.take();
        let entity = cx.entity().downgrade();
        let owner = self.owner.clone();
        div().size_full().relative().child(content).child(
            canvas(
                |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                move |_, hitbox, window, cx| {
                    if let Some((scroll, index, fraction)) = pending
                        && let Some(row) = scroll.bounds_for_item(index)
                    {
                        let offset = row.size.height * fraction;
                        let current = scroll.logical_scroll_top();
                        if current.item_ix == index
                            && (current.offset_in_item - offset).abs() > px(0.5)
                        {
                            // Apply once: the list may clamp near EOF, and retrying that
                            // clamped position every frame would cause a redraw loop.
                            window.defer(cx, move |_, cx| {
                                // Navigation or a newer source anchor can update
                                // this shared list after paint queued the fix.
                                // An old correction must not restore its item.
                                let latest = scroll.logical_scroll_top();
                                if latest.item_ix != current.item_ix
                                    || latest.offset_in_item != current.offset_in_item
                                {
                                    return;
                                }
                                scroll.scroll_to(ListOffset {
                                    item_ix: index,
                                    offset_in_item: offset,
                                });
                                let _ = entity.update(cx, |_, cx| cx.notify());
                            });
                        }
                    }
                    window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                        if phase.capture() && hitbox.should_handle_scroll(window) {
                            let _ = owner.update(cx, |workspace, cx| {
                                if let Some(tab) = workspace.tabs.get_mut(workspace.active) {
                                    tab.pending_anchor = None;
                                }
                                if workspace.scroll_locked
                                    && workspace.mode == ViewMode::Split
                                    && let Some(tab) = workspace.tabs.get(workspace.active)
                                {
                                    tab.editor.update(cx, |editor, cx| {
                                        editor.scroll_by(-event.delta.pixel_delta(px(20.)).y, cx);
                                    });
                                    cx.stop_propagation();
                                }
                            });
                        }
                    });
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
    }
}

#[derive(Clone)]
pub(super) struct ConnectionSnapshot {
    pub(super) revision: (u64, u64, bool, bool),
    pub(super) items: Arc<Vec<Connection>>,
    pub(super) backlinks: usize,
    pub(super) tags: usize,
    pub(super) empty_message: &'static str,
}

pub(super) enum Connection {
    Note(PathBuf, String),
    Tag(String),
}
pub(super) struct Connections {
    #[cfg(test)]
    pub(super) render_count: usize,
    pub(super) owner: WeakEntity<Workspace>,
    pub(super) items: Arc<Vec<Connection>>,
    pub(super) backlinks: usize,
    pub(super) tags: usize,
    pub(super) empty_message: &'static str,
    pub(super) scroll: UniformListScrollHandle,
}
impl Render for Connections {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.render_count += 1;
        }
        let mut header = div()
            .h(px(30.))
            .px_5()
            .flex()
            .items_center()
            .gap_3()
            .child(caption(format!("BACKLINKS · {}", self.backlinks)));
        if self.backlinks == 0 {
            header = header.child(caption(self.empty_message));
        }
        if self.tags > 0 {
            header = header.child(caption(format!("TAGS · {}", self.tags)));
        }
        div()
            .size_full()
            .border_t_1()
            .border_color(rgb(LINE))
            .flex()
            .flex_col()
            .child(header)
            .when(!self.items.is_empty(), |el| {
                el.child(
                    uniform_list(
                        "connections",
                        self.items.len().div_ceil(3),
                        cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                            range
                                .map(|row| {
                                    let mut line = div().h(px(30.)).flex().gap_2().px_4();
                                    for index in row * 3..((row + 1) * 3).min(this.items.len()) {
                                        let (label, color) = match &this.items[index] {
                                            Connection::Note(_, title) => {
                                                (format!("↗ {title}"), ACCENT)
                                            }
                                            Connection::Tag(tag) => (format!("#{tag}"), 0x9bbfb3),
                                        };
                                        line = line.child(
                                            button(("connection", index), label)
                                                .debug_selector(move || {
                                                    format!("connection-{index}")
                                                })
                                                .flex_1()
                                                .min_w_0()
                                                .truncate()
                                                .text_color(rgb(color))
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| match &this.items
                                                        [index]
                                                    {
                                                        Connection::Note(path, _) => {
                                                            let path = path.clone();
                                                            let _ = this.owner.update(
                                                                cx,
                                                                |workspace, cx| {
                                                                    workspace
                                                                        .open_note(path, window, cx)
                                                                },
                                                            );
                                                        }
                                                        Connection::Tag(tag) => {
                                                            let tag = tag.clone();
                                                            let _ = this.owner.update(
                                                                cx,
                                                                |workspace, cx| {
                                                                    workspace.filter_tag(
                                                                        &tag, window, cx,
                                                                    )
                                                                },
                                                            );
                                                        }
                                                    },
                                                )),
                                        );
                                    }
                                    line.into_any_element()
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(self.scroll.clone())
                    .flex_1()
                    .min_h_0(),
                )
            })
    }
}

pub(super) struct Sidebar {
    #[cfg(test)]
    pub(super) render_count: usize,
    pub(super) owner: WeakEntity<Workspace>,
}
impl Render for Sidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.render_count += 1;
        }
        self.owner
            .update(cx, |workspace, cx| workspace.render_sidebar(cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}
