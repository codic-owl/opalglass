// Command palette, bookmarks, daily notes and templates.
use super::*;

impl Workspace {
    pub(super) fn show_picker(
        &mut self,
        mode: PickerMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.managing || self.loading {
            return;
        }
        if mode != PickerMode::Commands && self.vault.is_none() {
            return;
        }
        if mode == PickerMode::Templates && self.tabs.is_empty() {
            return;
        }
        self.dismiss(window, cx);
        self.settings_open = false;
        self.close_completion(cx);
        self.picker = Some(mode);
        self.picker_query = None;
        self.picker_selected = 0;
        self.input.update(cx, |e, cx| e.set_text("", cx));
        self.input.read(cx).focus.focus(window);
        cx.notify();
    }
    pub(super) fn update_picker(&mut self, cx: &App) {
        let Some(mode) = self.picker else {
            return;
        };
        let query = self.input.read(cx).buffer.text().trim().to_lowercase();
        if self.picker_query.as_ref() == Some(&query) {
            return;
        }
        let matches = |label: &str| {
            let label = label.to_lowercase();
            query.split_whitespace().all(|word| label.contains(word))
        };
        self.picker_items = match mode {
            PickerMode::Commands => opalglass::commands::Command::ALL
                .iter()
                .filter(|(command, label, _)| {
                    command.available(self.vault.is_some(), !self.tabs.is_empty()) && matches(label)
                })
                .map(|(command, label, shortcut)| PickerItem {
                    title: (*label).into(),
                    detail: (*shortcut).into(),
                    action: PickerAction::Command(*command),
                })
                .collect(),
            PickerMode::Bookmarks => self
                .navigation
                .bookmarks
                .iter()
                .filter(|p| {
                    self.vault.as_ref().is_some_and(|v| v.contains_path(p))
                        && matches(&p.to_string_lossy())
                })
                .map(|p| PickerItem {
                    title: p.file_stem().unwrap_or_default().to_string_lossy().into(),
                    detail: p.display().to_string(),
                    action: PickerAction::Note(p.clone()),
                })
                .collect(),
            PickerMode::Templates => self
                .vault
                .as_ref()
                .into_iter()
                .flat_map(|v| v.notes.iter())
                .filter(|n| n.path.starts_with("Templates") && matches(&n.path.to_string_lossy()))
                .map(|n| PickerItem {
                    title: n.title.clone(),
                    detail: n.path.display().to_string(),
                    action: PickerAction::Template(n.path.clone()),
                })
                .collect(),
        };
        self.picker_selected = 0;
        self.picker_query = Some(query);
        self.picker_scroll.scroll_to_item(0, ScrollStrategy::Top);
    }
    pub(super) fn accept_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.picker_items.get(self.picker_selected).cloned() else {
            return;
        };
        self.picker = None;
        self.focus_document(window, cx);
        match item.action {
            PickerAction::Command(command) => self.execute_command(command, window, cx),
            PickerAction::Note(path) => self.open_note(path, window, cx),
            PickerAction::Template(path) => self.insert_template(path, cx),
        }
        cx.notify();
    }
    pub(super) fn picker_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let mode = self.picker.unwrap();
        let (title, empty) = match mode {
            PickerMode::Commands => ("Command palette", "No matching commands"),
            PickerMode::Bookmarks => ("Bookmarks", "Bookmark a note with Ctrl Shift D"),
            PickerMode::Templates => (
                "Insert template",
                "Create Markdown templates in the Templates folder",
            ),
        };
        div()
            .absolute()
            .inset_0()
            .occlude()
            .bg(rgba(0x05070bb0))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(560.))
                    .p_4()
                    .rounded_lg()
                    .bg(rgb(PANEL))
                    .border_1()
                    .border_color(rgb(LINE))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_lg().child(title))
                    .child(
                        div()
                            .h(px(40.))
                            .border_1()
                            .border_color(rgb(ACCENT))
                            .rounded_md()
                            .bg(rgb(BG))
                            .child(self.input.clone()),
                    )
                    .when(self.picker_items.is_empty(), |el| el.child(caption(empty)))
                    .child(
                        uniform_list(
                            "quick-picker",
                            self.picker_items.len(),
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .map(|index| {
                                        let item = &this.picker_items[index];
                                        div()
                                            .id(("picker-option", index))
                                            .w_full()
                                            .debug_selector(move || {
                                                format!("picker-option-{index}")
                                            })
                                            .h(px(46.))
                                            .px_3()
                                            .rounded_md()
                                            .flex()
                                            .flex_col()
                                            .justify_center()
                                            .cursor_pointer()
                                            .when(index == this.picker_selected, |el| {
                                                el.bg(rgb(0x303749))
                                            })
                                            .hover(|s| s.bg(rgb(0x303749)))
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .truncate()
                                                    .child(item.title.clone()),
                                            )
                                            .child(caption(item.detail.clone()).truncate())
                                            .on_click(cx.listener(move |this, _, w, cx| {
                                                this.picker_selected = index;
                                                this.accept_picker(w, cx);
                                            }))
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .h(px((self.picker_items.len().clamp(1, 8) * 46) as f32))
                        .track_scroll(self.picker_scroll.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(caption("↑ ↓ to choose · Enter to open · Escape to close"))
                            .child(button("picker-close", "Close").on_click(cx.listener(
                                |this, _, w, cx| {
                                    this.picker = None;
                                    this.focus_document(w, cx);
                                    cx.notify();
                                },
                            ))),
                    ),
            )
            .into_any_element()
    }
    pub(super) fn toggle_bookmark(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.tabs.get(self.active).map(|t| t.path.clone()) else {
            return;
        };
        if self.navigation.bookmarks.contains(&path) {
            self.navigation.bookmarks.retain(|p| p != &path);
            self.message("Bookmark removed", false, cx);
        } else {
            self.navigation.bookmarks.push(path);
            self.message("Note bookmarked", false, cx);
        }
        self.persist_navigation(cx);
    }
    pub(super) fn execute_command(
        &mut self,
        command: opalglass::commands::Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use opalglass::commands::Command as C;
        match command {
            C::OpenVault => self.pick_vault(window, cx),
            C::NewNote => self.show_inline(Modal::New, window, cx),
            C::NewFolder => self.show_inline(Modal::NewFolder, window, cx),
            C::Save => {
                self.save_tab(self.active, cx);
            }
            C::Find => self.open_find(window, cx),
            C::Properties => self.open_properties(window, cx),
            C::Mentions => self.open_mentions(window, cx),
            C::History => self.open_history(window, cx),
            C::UndoReplace => self.undo_replacements(window, cx),
            C::Compare => self.open_comparison(window, cx),
            C::Replace => self.open_replace(window, cx),
            C::Search => {
                self.navigation.hidden = false;
                self.search.update(cx, |e, cx| {
                    e.buffer.select(0, e.buffer.text().len());
                    cx.notify();
                });
                self.search.read(cx).focus.focus(window);
            }
            C::Source => self.set_mode(ViewMode::Source, window, cx),
            C::Split => self.set_mode(ViewMode::Split, window, cx),
            C::Reading => self.set_mode(ViewMode::Preview, window, cx),
            C::LivePreview => self.toggle_live_preview(window, cx),
            C::Outline => {
                self.outline_open = !self.outline_open;
            }
            C::Settings => {
                self.settings_open = true;
                self.focus.focus(window);
            }
            C::Back => self.visit_history(false, window, cx),
            C::Forward => self.visit_history(true, window, cx),
            C::Bookmark => self.toggle_bookmark(cx),
            C::Bookmarks => self.show_picker(PickerMode::Bookmarks, window, cx),
            C::DailyNote => self.open_daily_note(window, cx),
            C::Template => self.show_picker(PickerMode::Templates, window, cx),
            C::Trash => {
                self.navigation.selected = self.tabs.get(self.active).map(|t| t.path.clone());
                self.show_modal(Modal::Delete, window, cx);
            }
            C::RestoreTrash => self.change_trash(None, window, cx),
            C::Bold | C::Italic | C::Code | C::Task => {
                if let Some(tab) = self.tabs.get(self.active) {
                    tab.editor.update(cx, |e, cx| {
                        match command {
                            C::Bold => opalglass::editing::wrap(&mut e.buffer, "**"),
                            C::Italic => opalglass::editing::wrap(&mut e.buffer, "*"),
                            C::Code => opalglass::editing::wrap(&mut e.buffer, "`"),
                            _ => opalglass::editing::toggle_task(&mut e.buffer),
                        }
                        e.reveal_cursor(cx);
                    });
                }
            }
        }
        cx.notify();
    }
    pub(super) fn open_daily_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.managing || self.checkpointing {
            return;
        }
        let Some(mut vault) = self.vault.clone() else {
            return;
        };
        self.managing = true;
        self.cancel_indexing();
        self.note_generation += 1;
        self._note_load = None;
        self.note_loading = false;
        self.revision += 1;
        self.focus.focus(window);
        self.message("Opening today's note…", false, cx);
        self._management = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut values = opalglass::commands::TemplateValues::now(String::new())?;
                    values.title = values.date.clone();
                    let path = opalglass::commands::daily_note(&mut vault, &values)?;
                    Ok::<_, std::io::Error>((vault, path))
                })
                .await;
            let _ = this.update_in(cx, |this, w, cx| {
                this.managing = false;
                match result {
                    Ok((vault, path)) => {
                        this.vault = Some(vault);
                        this.revision += 1;
                        this.open_note(path, w, cx);
                    }
                    Err(error) => {
                        this.focus_document(w, cx);
                        this.message(format!("Could not open daily note: {error}"), true, cx);
                    }
                }
            });
        }));
    }
    pub(super) fn insert_template(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(vault) = self.vault.clone() else {
            return;
        };
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let editor = tab.editor.clone();
        let revision = editor.read(cx).buffer.revision();
        let range = editor.read(cx).buffer.selection();
        let title = tab
            .path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let template = vault.read(&path)?;
                    let values = opalglass::commands::TemplateValues::now(title)?;
                    Ok::<_, std::io::Error>(opalglass::document::normalize(
                        &opalglass::commands::render_template(&template, &values),
                    ))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .tabs
                    .get(this.active)
                    .is_none_or(|t| t.editor != editor)
                    || editor.read(cx).buffer.revision() != revision
                    || editor.read(cx).buffer.selection() != range
                {
                    this.message(
                        "The insertion point changed. Choose the template again.",
                        false,
                        cx,
                    );
                    return;
                }
                match result {
                    Ok(text) => {
                        editor.update(cx, |e, cx| {
                            e.buffer.replace(range, &text);
                            e.reveal_cursor(cx);
                        });
                        this.message("Template inserted", false, cx);
                    }
                    Err(error) => {
                        this.message(format!("Could not insert template: {error}"), true, cx)
                    }
                }
            });
        })
        .detach();
    }
}
