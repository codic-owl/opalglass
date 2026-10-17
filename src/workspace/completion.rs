// Asynchronous wiki-link completion and its menu.
use super::*;

impl Workspace {
    pub(super) fn close_completion(&mut self, cx: &mut Context<Self>) {
        self.completion.candidates.clear();
        self.completion.key = None;
        self.completion.generation += 1;
        self.completion.task = None;
        self.completion.cancelled.store(true, Ordering::Relaxed);
        for tab in &self.tabs {
            tab.editor.update(cx, |e, _| e.completion_visible = false);
        }
        cx.notify();
    }
    pub(super) fn update_completion(&mut self, window: &Window, cx: &mut Context<Self>) {
        let context = self
            .tabs
            .get(self.active)
            .filter(|t| t.editor.read(cx).focus.is_focused(window))
            .filter(|_| {
                self.modal == Modal::None
                    && !self.settings_open
                    && !self.managing
                    && self.mode != ViewMode::Preview
            })
            .and_then(|t| {
                t.editor
                    .read(cx)
                    .link_query()
                    .map(|(range, query)| (t.editor.clone(), t.path.clone(), range, query))
            });
        let Some((editor, from, range, query)) = context else {
            if self.completion.key.is_some() {
                self.close_completion(cx);
            }
            return;
        };
        let key = (
            editor.entity_id(),
            editor.read(cx).buffer.revision(),
            editor.read(cx).buffer.cursor(),
        );
        if self.completion.key == Some(key) || self.completion.dismissed == Some(key) {
            return;
        }
        self.close_completion(cx);
        self.completion.key = Some(key);
        self.completion.range = range;
        let generation = self.completion.generation;
        self.completion.cancelled = Arc::new(AtomicBool::new(false));
        let cancelled = self.completion.cancelled.clone();
        let Some(vault) = self.vault.clone() else {
            return;
        };
        let engine = self.search_engine.clone();
        self.completion.task = Some(cx.spawn(async move |this, cx| {
            let candidates = cx
                .background_executor()
                .spawn(async move {
                    let q = query.to_lowercase();
                    let mut candidates = Vec::new();
                    if !q.is_empty()
                        && let Some(engine) = engine
                        && let Ok(results) =
                            engine.search(&query, &Ticket::from_cancelled(cancelled.clone()))
                    {
                        candidates.extend(
                            results
                                .hits
                                .into_iter()
                                .filter(|h| h.tier.contains("title"))
                                .map(|h| (PathBuf::from(h.path), h.title)),
                        );
                    }
                    let mut fallback = vault
                        .notes
                        .iter()
                        .filter_map(|n| {
                            if n.path == from {
                                return None;
                            }
                            let title = n.title.to_lowercase();
                            let path = n.path.to_string_lossy().to_lowercase();
                            (title.contains(&q) || path.contains(&q)).then_some((
                                !title.starts_with(&q),
                                title,
                                n.path.clone(),
                                n.title.clone(),
                            ))
                        })
                        .collect::<Vec<_>>();
                    fallback.sort();
                    for (_, _, path, title) in fallback {
                        if candidates.len() >= 10 {
                            break;
                        }
                        if !candidates.iter().any(|(p, _)| *p == path) {
                            candidates.push((path, title));
                        }
                    }
                    candidates.retain(|(p, _)| *p != from && vault.contains_path(p));
                    candidates.truncate(10);
                    candidates
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.completion.generation != generation || this.completion.key != Some(key) {
                    return;
                }
                editor.update(cx, |e, _| e.completion_visible = !candidates.is_empty());
                this.completion.candidates = candidates;
                this.completion.selected = 0;
                cx.notify();
            });
        }));
    }
    pub(super) fn accept_completion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, title)) = self
            .completion
            .candidates
            .get(self.completion.selected)
            .cloned()
        else {
            return;
        };
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        if self.completion.key
            != Some((
                tab.editor.entity_id(),
                tab.editor.read(cx).buffer.revision(),
                tab.editor.read(cx).buffer.cursor(),
            ))
        {
            return;
        }
        let Some(vault) = &self.vault else {
            return;
        };
        let target = if vault.resolve(&tab.path, &title).as_ref() == Some(&path) {
            title
        } else {
            format!(
                "/{}",
                path.with_extension("").to_string_lossy().replace('\\', "/")
            )
        };
        let range = self.completion.range.clone();
        tab.editor.update(cx, |e, cx| {
            e.buffer.replace(range, &format!("[[{target}]]"));
            e.jump_to(e.buffer.cursor(), cx);
            e.focus.focus(window);
        });
        self.close_completion(cx);
    }
    pub(super) fn completion_menu(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let tab = &self.tabs[self.active];
        let point = tab.editor.read(cx).caret_position();
        let size = window.viewport_size();
        let height = self.completion.candidates.len() as f32 * 42. + 8.;
        let top = if f32::from(point.y) + 30. + height < f32::from(size.height) {
            f32::from(point.y) + 30.
        } else {
            (f32::from(point.y) - height).max(0.)
        };
        div()
            .id("link-completion")
            .debug_selector(|| "link-completion".into())
            .absolute()
            .left(px(
                f32::from(point.x).clamp(0., (f32::from(size.width) - 340.).max(0.))
            ))
            .top(px(top))
            .w(px(340.))
            .py_1()
            .rounded_md()
            .bg(rgb(PANEL))
            .border_1()
            .border_color(rgb(LINE))
            .shadow_lg()
            .occlude()
            .children(
                self.completion
                    .candidates
                    .iter()
                    .enumerate()
                    .map(|(i, (path, title))| {
                        div()
                            .id(("link-option", i))
                            .debug_selector(move || format!("link-option-{i}"))
                            .h(px(42.))
                            .px_3()
                            .cursor_pointer()
                            .when(i == self.completion.selected, |el| el.bg(rgb(0x303749)))
                            .hover(|s| s.bg(rgb(0x303749)))
                            .child(div().text_sm().truncate().child(title.clone()))
                            .child(caption(path.display().to_string()).truncate())
                            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                                window.prevent_default();
                                cx.stop_propagation();
                            })
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.completion.selected = i;
                                this.accept_completion(w, cx);
                            }))
                    }),
            )
            .into_any_element()
    }
}
