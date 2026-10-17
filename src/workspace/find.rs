// Find-in-note queries, navigation and controls.
use super::*;

impl Workspace {
    pub(super) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let selected = tab.editor.read(cx).buffer.selected().to_owned();
        self.find.open = true;
        self.find.input.update(cx, |input, cx| {
            if !selected.is_empty() && !selected.contains('\n') {
                input.set_text(&selected, cx);
            }
            input.buffer.select(0, input.buffer.text().len());
            input.focus.focus(window);
            cx.notify();
        });
        cx.notify();
    }
    pub(super) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find.open = false;
        self.find.generation += 1;
        self.find.task = None;
        self.find.key = None;
        self.find.matches = Arc::new(Vec::new());
        for tab in &self.tabs {
            tab.editor
                .update(cx, |e, cx| e.set_find_matches(Arc::new(Vec::new()), cx));
        }
        self.preview_pane.update(cx, |_, cx| cx.notify());
        self.focus_document(window, cx);
        cx.notify();
    }
    pub(super) fn update_find(&mut self, cx: &mut Context<Self>) {
        if !self.find.open {
            return;
        }
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let editor = tab.editor.clone();
        let query = self.find.input.read(cx).buffer.text().to_owned();
        let key = (
            editor.entity_id(),
            editor.read(cx).buffer.revision(),
            query.clone(),
            self.find.case_sensitive,
        );
        if self.find.key.as_ref() == Some(&key) {
            return;
        }
        self.find.key = Some(key);
        self.find.generation += 1;
        let generation = self.find.generation;
        let text = editor.read(cx).buffer.snapshot();
        let case_sensitive = self.find.case_sensitive;
        self.find.selected = None;
        self.find.matches = Arc::new(Vec::new());
        self.find.searching = !query.is_empty();
        editor.update(cx, |e, cx| e.set_find_matches(Arc::new(Vec::new()), cx));
        self.preview_pane.update(cx, |_, cx| cx.notify());
        self.find.task = Some(cx.spawn(async move |this, cx| {
            let matches =
                cx.background_executor()
                    .spawn(async move {
                        Arc::new(opalglass::editing::find(&text, &query, case_sensitive))
                    })
                    .await;
            let _ = this.update(cx, |this, cx| {
                if this.find.generation != generation {
                    return;
                }
                this.find.searching = false;
                let cursor = editor.read(cx).buffer.cursor();
                let first = matches.partition_point(|r| r.end <= cursor);
                this.find.selected =
                    (!matches.is_empty()).then_some(if first == matches.len() { 0 } else { first });
                editor.update(cx, |e, cx| e.set_find_matches(matches.clone(), cx));
                this.find.matches = matches;
                this.reveal_find(cx);
                this.preview_pane.update(cx, |_, cx| cx.notify());
                cx.notify();
            });
        }));
    }
    pub(super) fn reveal_find(&mut self, cx: &mut Context<Self>) {
        let Some(range) = self
            .find
            .selected
            .and_then(|i| self.find.matches.get(i))
            .cloned()
        else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        tab.editor
            .update(cx, |e, cx| e.select_match(range.clone(), cx));
        let row = tab.prepared.row_at_source(range.start);
        tab.reveal_preview_row(row);
        self.preview_pane.update(cx, |_, cx| cx.notify());
    }
    pub(super) fn next_find(&mut self, backwards: bool, cx: &mut Context<Self>) {
        let count = self.find.matches.len();
        if count == 0 {
            return;
        }
        let selected = self.find.selected.unwrap_or(0);
        self.find.selected = Some((selected + if backwards { count - 1 } else { 1 }) % count);
        self.reveal_find(cx);
        cx.notify();
    }
    pub(super) fn find_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .debug_selector(|| "note-find".into())
            .flex()
            .items_center()
            .gap_2()
            .px_4()
            .py_1()
            .border_b_1()
            .border_color(rgb(LINE))
            .flex_shrink_0()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(32.))
                    .child(self.find.input.clone()),
            )
            .child(caption(if self.find.searching {
                "Searching…".into()
            } else {
                format!(
                    "{} / {}",
                    self.find.selected.map_or(0, |i| i + 1),
                    self.find.matches.len()
                )
            }))
            .child(
                button("find-case", "Aa")
                    .tooltip({
                        let sensitive = self.find.case_sensitive;
                        move |_, cx| {
                            cx.new(|_| {
                                PathTooltip(format!(
                                    "Match case · {}",
                                    if sensitive { "On" } else { "Off" }
                                ))
                            })
                            .into()
                        }
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.find.case_sensitive = !this.find.case_sensitive;
                        cx.notify();
                    })),
            )
            .child(
                button("find-previous", "↑")
                    .debug_selector(|| "find-previous".into())
                    .tooltip(|_, cx| {
                        cx.new(|_| PathTooltip("Previous match · Shift Enter".into()))
                            .into()
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.next_find(true, cx))),
            )
            .child(
                button("find-next", "↓")
                    .debug_selector(|| "find-next".into())
                    .tooltip(|_, cx| cx.new(|_| PathTooltip("Next match · Enter".into())).into())
                    .on_click(cx.listener(|this, _, _, cx| this.next_find(false, cx))),
            )
            .child(
                button("find-close", "×")
                    .debug_selector(|| "find-close".into())
                    .on_click(cx.listener(|this, _, w, cx| this.close_find(w, cx))),
            )
            .into_any_element()
    }
}
