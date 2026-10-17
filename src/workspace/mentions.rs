// Mentions workflow and controls.
use super::*;

pub(super) struct MentionsPanel {
    pub(super) root: PathBuf,
    pub(super) target: PathBuf,
    pub(super) items: Arc<Vec<opalglass::changes::Mention>>,
    pub(super) loading: bool,
    pub(super) error: Option<String>,
    pub(super) scroll: UniformListScrollHandle,
    pub(super) cancelled: Arc<AtomicBool>,
}
impl Drop for MentionsPanel {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}
impl Workspace {
    pub(super) fn open_mentions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.managing {
            return;
        }
        let (Some(vault), Some(tab)) = (self.vault.clone(), self.tabs.get(self.active)) else {
            return;
        };
        let target = tab.path.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.mentions = Some(MentionsPanel {
            root: vault.root.clone(),
            target: target.clone(),
            items: Arc::new(vec![]),
            loading: true,
            error: None,
            scroll: UniformListScrollHandle::new(),
            cancelled: cancelled.clone(),
        });
        self.focus.focus(window);
        cx.notify();
        self._mentions = Some(cx.spawn(async move |this, cx| {
            let flag = cancelled.clone();
            let results = cx
                .background_executor()
                .spawn(async move { opalglass::changes::unlinked_mentions(&vault, &target, &flag) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(panel) = this
                    .mentions
                    .as_mut()
                    .filter(|p| Arc::ptr_eq(&p.cancelled, &cancelled))
                else {
                    return;
                };
                panel.loading = false;
                match results {
                    Ok(items) => panel.items = Arc::new(items),
                    Err(error) => panel.error = Some(error),
                }
                cx.notify();
            });
        }));
    }
    pub(super) fn link_mention(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.managing || self.checkpointing {
            return;
        }
        let Some(panel) = &self.mentions else {
            return;
        };
        let Some(mention) = panel.items.get(index).cloned() else {
            return;
        };
        let Some(mut vault) = self.vault.clone().filter(|v| v.root == panel.root) else {
            return;
        };
        if self
            .tabs
            .iter()
            .any(|t| t.path == mention.path && t.dirty(cx))
        {
            self.mentions.as_mut().unwrap().error = Some(
                "The mentioning note has unsaved edits. Open and save it, then refresh mentions."
                    .into(),
            );
            cx.notify();
            return;
        }
        let target = panel.target.clone();
        self.cancel_indexing();
        self.managing = true;
        self.message("Linking mention…", false, cx);
        self._management = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    opalglass::changes::link_mention(&mut vault, &mention, &target)?;
                    Ok::<_, String>(vault)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.managing = false;
                match result {
                    Ok(vault) => {
                        this.apply_scan(vault, this.revision, cx);
                        this.open_mentions(window, cx);
                        this.message("Mention linked", false, cx);
                    }
                    Err(error) => {
                        if let Some(panel) = &mut this.mentions {
                            panel.error = Some(error.clone());
                        }
                        this.message(error, true, cx);
                    }
                }
            });
        }));
    }
    pub(super) fn render_mentions(&self, cx: &mut Context<Self>) -> AnyElement {
        let panel = self.mentions.as_ref().unwrap();
        div()
            .absolute()
            .inset_0()
            .occlude()
            .bg(rgba(0x05070bb0))
            .p_6()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(780.))
                    .max_w(relative(0.95))
                    .h(relative(0.85))
                    .p_4()
                    .rounded_lg()
                    .bg(rgb(PANEL))
                    .border_1()
                    .border_color(rgb(LINE))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_lg().child("Unlinked mentions"))
                    .child(caption(panel.target.display().to_string()))
                    .child(caption(if panel.loading {
                        "Finding mentions…".into()
                    } else {
                        format!(
                            "{} mentions · saved notes · links, code, and properties excluded",
                            panel.items.len()
                        )
                    }))
                    .when(!panel.loading && panel.items.is_empty(), |el| {
                        el.child(caption("No unlinked mentions found."))
                    })
                    .child(
                        uniform_list(
                            "unlinked-mentions",
                            panel.items.len(),
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                let Some(panel) = &this.mentions else {
                                    return vec![];
                                };
                                range
                                    .map(|index| {
                                        let mention = &panel.items[index];
                                        let path = mention.path.clone();
                                        div()
                                            .h(px(82.))
                                            .py_2()
                                            .border_b_1()
                                            .border_color(rgb(LINE))
                                            .flex()
                                            .gap_3()
                                            .items_center()
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .flex()
                                                    .flex_col()
                                                    .gap_1()
                                                    .child(
                                                        button(
                                                            ("mention-note", index),
                                                            format!(
                                                                "{}:{}",
                                                                path.display(),
                                                                mention.line
                                                            ),
                                                        )
                                                        .on_click(cx.listener(
                                                            move |this, _, window, cx| {
                                                                this.mentions = None;
                                                                this.open_note(
                                                                    path.clone(),
                                                                    window,
                                                                    cx,
                                                                );
                                                            },
                                                        )),
                                                    )
                                                    .child(
                                                        div().text_sm().truncate().child(
                                                            StyledText::new(SharedString::from(
                                                                mention.context.clone(),
                                                            ))
                                                            .with_highlights([(
                                                                mention.highlight.clone(),
                                                                HighlightStyle {
                                                                    color: Some(rgb(ACCENT).into()),
                                                                    font_weight: Some(
                                                                        FontWeight::BOLD,
                                                                    ),
                                                                    ..Default::default()
                                                                },
                                                            )]),
                                                        ),
                                                    ),
                                            )
                                            .child(
                                                button(("link-mention", index), "Link")
                                                    .debug_selector(move || {
                                                        format!("link-mention-{index}")
                                                    })
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            this.link_mention(index, window, cx)
                                                        },
                                                    )),
                                            )
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(panel.scroll.clone())
                        .flex_1()
                        .min_h_0(),
                    )
                    .children(panel.error.as_ref().map(|error| {
                        div()
                            .text_sm()
                            .text_color(rgb(0xf1b18e))
                            .child(error.clone())
                    }))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(button("refresh-mentions", "Refresh").on_click(
                                cx.listener(|this, _, window, cx| this.open_mentions(window, cx)),
                            ))
                            .child(button("close-mentions", "Close").on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.mentions = None;
                                    this._mentions = None;
                                    this.focus_document(window, cx);
                                    cx.notify();
                                },
                            ))),
                    ),
            )
            .into_any_element()
    }
}
