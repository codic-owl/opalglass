struct HistoryPanel {
    root: PathBuf,
    path: PathBuf,
    editor: Entity<Editor>,
    revision: u64,
    versions: Vec<opalglass::history::Version>,
    active: usize,
    text: Option<String>,
    rows: Arc<Vec<opalglass::comparison::Row>>,
    scroll: UniformListScrollHandle,
    error: Option<String>,
}
impl Workspace {
    fn open_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.managing || self.checkpointing { return; }
        let (Some(vault), Some(tab)) = (&self.vault, self.tabs.get(self.active)) else { return; };
        let root = vault.root.clone();
        let path = tab.path.clone();
        let editor = tab.editor.clone();
        let revision = editor.read(cx).buffer.revision();
        self.focus.focus(window);
        self.message("Loading saved versions…", false, cx);
        self._history = Some(cx.spawn(async move |this, cx| {
            let (read_root, read_path) = (root.clone(), path.clone());
            let versions = cx.background_executor().spawn(async move { opalglass::history::versions(&read_root, &read_path) }).await;
            let _ = this.update(cx, |this, cx| {
                if this.vault.as_ref().is_none_or(|v| v.root != root)
                    || this.tabs.get(this.active).is_none_or(|t| t.editor != editor || t.path != path) { return; }
                match versions {
                    Ok(versions) => {
                        this.history_panel = Some(HistoryPanel { root, path, editor, revision, versions, active: 0, text: None, rows: Arc::new(vec![]), scroll: UniformListScrollHandle::new(), error: None });
                        this.select_history(0, cx);
                        this.message("Saved versions loaded", false, cx);
                    }
                    Err(error) => this.message(format!("Could not load saved versions: {error}"), true, cx),
                }
                cx.notify();
            });
        }));
    }
    fn select_history(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(panel) = &mut self.history_panel else { return; };
        let Some(version) = panel.versions.get(index) else { return; };
        panel.active = index;
        panel.text = None;
        panel.error = None;
        panel.rows = Arc::new(vec![]);
        let (root, path, id) = (panel.root.clone(), panel.path.clone(), version.id.clone());
        let ours = panel.editor.read(cx).buffer.text().to_owned();
        let editor = panel.editor.clone();
        self._history = Some(cx.spawn(async move |this, cx| {
            let (read_root, read_path) = (root.clone(), path.clone());
            let result = cx.background_executor().spawn(async move {
                let text = opalglass::history::read(&read_root, &read_path, &id)?;
                let rows = opalglass::comparison::compare(&ours, &opalglass::document::normalize(&text));
                Ok::<_, std::io::Error>((text, rows))
            }).await;
            let _ = this.update(cx, |this, cx| {
                let Some(panel) = &mut this.history_panel else { return; };
                if panel.root != root || panel.path != path || panel.editor != editor || panel.active != index { return; }
                match result {
                    Ok((text, rows)) => { panel.text = Some(text); panel.rows = Arc::new(rows); panel.scroll = UniformListScrollHandle::new(); }
                    Err(e) => panel.error = Some(e.to_string()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn restore_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = &self.history_panel else { return; };
        let Some(text) = &panel.text else { return; };
        if panel.editor.read(cx).buffer.revision() != panel.revision {
            self.history_panel.as_mut().unwrap().error = Some("The note changed while reviewing. Reopen saved versions before restoring.".into());
            cx.notify(); return;
        }
        let text = opalglass::document::normalize(text);
        panel.editor.update(cx, |editor, cx| {
            editor.buffer.replace(0..editor.buffer.text().len(), &text);
            editor.reveal_cursor(cx);
        });
        self.history_panel = None;
        self.set_mode(ViewMode::Source, window, cx);
        self.message("Saved version restored into the editor. Undo is available.", false, cx);
    }
    fn undo_replacements(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.managing || self.checkpointing { return; }
        if self.tabs.iter().any(|t| t.dirty(cx)) {
            self.message("Save open edits before undoing a bulk replacement.", true, cx); return;
        }
        let Some(mut vault) = self.vault.clone() else { return; };
        self.cancel_indexing(); self.managing = true; self.focus.focus(window);
        self.message("Restoring notes from the last bulk replacement…", false, cx);
        self._management = Some(cx.spawn(async move |this, cx| {
            let (vault, result) = cx.background_executor().spawn(async move {
                let result = opalglass::history::undo_latest(&mut vault);
                (vault, result)
            }).await;
            let _ = this.update(cx, |this, cx| {
                this.managing = false;
                this.apply_scan(vault, this.revision, cx);
                match result {
                    Ok(count) => this.message(format!("Restored {count} note(s) from the last bulk replacement"), false, cx),
                    Err(error) => this.message(error, true, cx),
                }
            });
        }));
    }
    fn render_history(&self, cx: &mut Context<Self>) -> AnyElement {
        let panel = self.history_panel.as_ref().unwrap();
        let rows = panel.rows.clone();
        div().absolute().inset_0().occlude().bg(rgba(0x05070bb0)).p_6().flex().items_center().justify_center()
            .child(div().w_full().h_full().max_w(px(1200.)).p_4().rounded_lg().bg(rgb(PANEL)).border_1().border_color(rgb(LINE)).flex().flex_col().gap_3()
                .child(div().text_lg().child("Saved versions"))
                .child(caption(panel.path.display().to_string()))
                .child(caption("− Current editor text    + Selected saved version. ↑/↓ choose · Enter restores · Esc closes. Restoring is undoable."))
                .when(panel.versions.is_empty(), |el| el.child(caption("No saved versions yet. Versions are captured before a note is overwritten.")))
                .child(div().flex_1().min_h_0().flex().gap_3()
                    .child(uniform_list("history-versions", panel.versions.len(), cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        let panel = this.history_panel.as_ref().unwrap();
                        range.map(|index| {
                            let version = &panel.versions[index];
                            let date = version.label();
                            button(("history-version", index), date).h(px(36.)).w_full()
                                .debug_selector(move || format!("history-version-{index}"))
                                .when(panel.active == index, |el| el.bg(rgb(0x303449)))
                                .on_click(cx.listener(move |this, _, _, cx| this.select_history(index, cx)))
                        }).collect::<Vec<_>>()
                    })).w(px(280.)).h_full())
                    .child(uniform_list("history-lines", rows.len(), cx.processor(move |_, range: std::ops::Range<usize>, _, _| {
                        range.map(|index| {
                            use opalglass::comparison::Kind;
                            let row = &rows[index];
                            let (marker, background) = match row.kind { Kind::Added => ("+", 0x20372f), Kind::Removed => ("−", 0x39282e), Kind::Divider => ("⋯", PANEL), _ => (" ", BG) };
                            div().h(px(26.)).px_2().bg(rgb(background)).text_sm().font_family(MONO_FONT).truncate().child(format!("{marker} {}", row.text))
                        }).collect::<Vec<_>>()
                    })).track_scroll(panel.scroll.clone()).flex_1().min_w_0().h_full()))
                .children(panel.error.iter().map(|e| caption(e.clone())))
                .child(div().flex().justify_end().gap_2()
                    .child(button("history-close", "Close").debug_selector(|| "history-close".into()).on_click(cx.listener(|this, _, w, cx| { this.history_panel = None; this.focus_document(w, cx); cx.notify(); })))
                    .when(panel.text.is_some(), |el| el.child(button("history-restore", "Restore selected version").debug_selector(|| "history-restore".into()).on_click(cx.listener(|this, _, w, cx| this.restore_history(w, cx)))))))
            .into_any_element()
    }
}
