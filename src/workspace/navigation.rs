// Opening notes and restoring navigation history.
use super::*;

#[path = "navigation/destination.rs"]
mod destination;
use destination::Destination;

impl Workspace {
    pub(super) fn current_location(&self, cx: &App) -> Option<navigation::SessionTab> {
        let tab = self.tabs.get(self.active)?;
        let editor = tab.editor.read(cx);
        let preview = tab
            .pending_preview
            .unwrap_or_else(|| tab.saved_preview_offset());
        Some(navigation::SessionTab {
            path: tab.path.clone(),
            cursor: editor.buffer.cursor(),
            anchor: editor.buffer.anchor(),
            source: editor.saved_scroll(),
            preview: navigation::ScrollPosition {
                row: preview.item_ix,
                offset: preview.offset_in_item.into(),
            },
        })
    }
    pub(super) fn remember_departure(&mut self, cx: &App) {
        if self.history_request {
            return;
        }
        self.history_restore = None;
        if let Some(location) = self.current_location(cx) {
            if self.history_back.last() != Some(&location) {
                self.history_back.push(location);
            }
            if self.history_back.len() > 100 {
                self.history_back.remove(0);
            }
            self.history_forward.clear();
        }
    }
    pub(super) fn apply_history_position(&mut self, cx: &mut Context<Self>) {
        let Some(saved) = self.history_restore.take() else {
            return;
        };
        let Some(tab) = self
            .tabs
            .get_mut(self.active)
            .filter(|t| t.path == saved.path)
        else {
            return;
        };
        tab.editor.update(cx, |e, cx| {
            e.restore_position(saved.anchor, saved.cursor, saved.source, cx)
        });
        let pos = ListOffset {
            item_ix: saved.preview.row,
            offset_in_item: px(saved.preview.offset),
        };
        if tab.prepared.complete {
            tab.restore_preview_offset(pos);
        } else {
            tab.pending_preview = Some(pos);
        }
        self.preview_pane.update(cx, |_, cx| cx.notify());
        cx.notify();
    }
    pub(super) fn visit_history(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.managing || self.modal != Modal::None || self.settings_open {
            return;
        }
        let current = self.current_location(cx);
        let stack = if forward {
            &mut self.history_forward
        } else {
            &mut self.history_back
        };
        let target = loop {
            let Some(target) = stack.pop() else {
                cx.notify();
                return;
            };
            if self
                .vault
                .as_ref()
                .is_some_and(|v| v.contains_path(&target.path))
            {
                break target;
            }
        };
        if let Some(current) = current {
            if forward {
                self.history_back.push(current);
            } else {
                self.history_forward.push(current);
            }
        }
        self.history_restore = Some(target.clone());
        self.history_request = true;
        self.open_note(target.path, window, cx);
        self.history_request = false;
    }
    pub(super) fn open_note(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_note_at(path, None, window, cx);
    }
    pub(super) fn open_note_at(
        &mut self,
        path: PathBuf,
        heading: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_note_to(path, heading.map(Destination::Fragment), window, cx);
    }
    fn open_note_to(
        &mut self,
        path: PathBuf,
        destination: Option<Destination>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(vault) = &self.vault
            && vault.files.contains(&path)
        {
            if !opalglass::attachments::supported(&path) {
                self.message("This attachment type cannot be opened yet", true, cx);
                return;
            }
            match vault.file_path(&path) {
                Ok(full) => cx.open_with_system(&full),
                Err(error) => self.message(format!("Could not open attachment: {error}"), true, cx),
            }
            return;
        }
        if !self.history_request {
            self.history_restore = None;
        }
        let started = Instant::now();
        if let Some(tab) = self.tabs.get_mut(self.active)
            && tab.path != path
        {
            tab.cancel_opening_analysis();
        }
        self.note_generation += 1;
        self._note_load = None;
        self.note_loading = false;
        if let Some(index) = self.tabs.iter().position(|t| t.path == path) {
            if destination.is_none() {
                self.activate_tab(index, window, cx);
            } else {
                self.remember_departure(cx);
                self.active = index;
                self.finish_note_navigation(destination, window, cx);
                self.log_performance("Follow heading", path.display().to_string(), started, cx);
            }
            return;
        }
        let Some(vault) = self.vault.clone() else {
            return;
        };
        let root = vault.root.clone();
        let directory = self.state_directory.clone();
        let generation = self.note_generation;
        self.modal = Modal::None;
        self.inline_parent = None;
        self.explorer_menu = None;
        self.sort_menu = None;
        self.focus.focus(window);
        self.message("Opening note…", false, cx);
        self.note_loading = true;
        self._note_load = Some(cx.spawn_in(window, async move |this, cx| {
            let read_path = path.clone();
            let (result, mut phases, worker_finished) = cx
                .background_executor()
                .spawn(async move {
                    let work = Instant::now();
                    let text = opalglass::state::load_document(&vault, &read_path, directory.as_deref());
                    let read = Instant::now();
                    let result = text.map(|(text, buffer)| {
                        let prepared = Arc::new(Prepared::opening(buffer.text()));
                        (text, buffer, prepared)
                    });
                    let finished = Instant::now();
                    (
                        result,
                        vec![
                            ("Worker queue", work.duration_since(started)),
                            ("Read", read.duration_since(work)),
                            ("Prepare", finished.duration_since(read)),
                        ],
                        finished,
                    )
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.note_generation != generation
                    || this.vault.as_ref().is_none_or(|v| v.root != root)
                {
                    return;
                }
                this.note_loading = false;
                let install = Instant::now();
                phases.push(("UI dispatch", install.duration_since(worker_finished)));
                let target = path.display().to_string();
                let action = if result.is_ok() {
                    "Open note"
                } else {
                    "Open failed"
                };
                match result {
                    Ok((text, buffer, prepared)) => {
                        if this.history_restore.is_none() { this.remember_departure(cx); }
                        this.install_tab(path, text, buffer, prepared, window, cx);
                        this.active = this.tabs.len() - 1;
                        if this.tabs[this.active].dirty(cx) {
                            this.message("Recovered unsaved edits. External changes will be checked before saving.", false, cx);
                        } else { this.message("Note opened", false, cx); }
                        this.finish_note_navigation(destination, window, cx);
                    }
                    Err(error) => this.message(format!("Could not open note: {error}"), true, cx),
                }
                phases.push(("UI install", install.elapsed()));
                this.log_performance_phases(action, target, started, phases, cx);
            });
        }));
    }
    pub(super) fn install_tab(
        &mut self,
        path: PathBuf,
        text: String,
        buffer: Buffer,
        prepared: Arc<Prepared>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let normalized = Arc::new(opalglass::document::normalize(&text));
        let dirty = normalized.as_str() != buffer.text();
        let revision = buffer.revision();
        let editor = cx.new(|cx| {
            let mut editor = Editor::from_buffer(buffer, false, "Start writing…", cx);
            editor.set_live_preview(self.live_preview, cx);
            editor.set_preferences(
                self.preferences.font_size,
                self.preferences.line_numbers,
                cx,
            );
            editor
        });
        let mut state = (revision, 0, 0);
        editor.update(cx, |editor, _| editor.accepts_attachments = true);
        let subscription = cx.observe(&editor, move |_, editor, cx| {
            let buffer = &editor.read(cx).buffer;
            let next = (buffer.revision(), buffer.cursor(), buffer.anchor());
            // Scrolling invalidates the editor's view, but doesn't
            // change the document, footer, or explorer.
            if state != next {
                state = next;
                cx.notify();
            }
        });
        let scroll_subscription = cx.subscribe(
            &editor,
            |this, editor, _: &crate::editor::ScrollAnchor, cx| {
                if this
                    .tabs
                    .get(this.active)
                    .is_some_and(|tab| tab.editor == editor)
                {
                    this.sync_reading_scroll(cx);
                }
            },
        );
        let completion_subscription = cx.subscribe_in(
            &editor,
            window,
            |this, _, action: &crate::editor::CompletionAction, window, cx| match action {
                crate::editor::CompletionAction::Accept => this.accept_completion(window, cx),
                crate::editor::CompletionAction::Dismiss => {
                    this.completion.dismissed = this.completion.key;
                    this.close_completion(cx);
                }
                _ => {
                    let count = this.completion.candidates.len();
                    if count > 0 {
                        let delta = if matches!(action, crate::editor::CompletionAction::Previous) {
                            count - 1
                        } else {
                            1
                        };
                        this.completion.selected = (this.completion.selected + delta) % count;
                        cx.notify();
                    }
                }
            },
        );
        let link_subscription = cx.subscribe_in(
            &editor,
            window,
            |this, _, link: &crate::editor::FollowLink, window, cx| {
                if external_link(&link.0) {
                    cx.open_url(&link.0);
                } else {
                    this.follow(link.0.clone(), window, cx);
                }
            },
        );
        let attachment_subscription = cx.subscribe_in(
            &editor,
            window,
            |this, editor, images: &crate::editor::PasteImages, window, cx| {
                let inputs = images
                    .0
                    .iter()
                    .map(|image| {
                        let extension = match image.format {
                            ImageFormat::Png => "png",
                            ImageFormat::Jpeg => "jpg",
                            ImageFormat::Webp => "webp",
                            ImageFormat::Gif => "gif",
                            ImageFormat::Svg => "svg",
                            ImageFormat::Bmp => "bmp",
                            ImageFormat::Tiff => "tiff",
                        };
                        opalglass::attachments::Input::Bytes {
                            name: format!("Screenshot.{extension}"),
                            bytes: image.bytes.clone(),
                        }
                    })
                    .collect();
                this.import_attachments(editor.clone(), inputs, window, cx);
            },
        );
        let complete = prepared.analysis_complete();
        let mut preview_projection = opalglass::callouts::Projection::default();
        preview_projection.rebuild(prepared.rows.len(), &prepared.callouts);
        preview_projection.with_footnotes(
            prepared.rows.len(),
            &prepared.footnotes,
            &prepared.callouts,
        );
        self.tabs.push(Tab {
            path,
            baseline: text,
            external_changed: false,
            baseline_normalized: normalized,
            dirty_cache: Cell::new((revision, dirty)),
            preview_scroll: ListState::new(
                preview_projection.len(prepared.rows.len()),
                ListAlignment::Top,
                px(100.),
            ),
            composition: reading_composition::State::new(
                preview_projection.len(prepared.rows.len()),
            ),
            preview_projection,
            connections_cache: None,
            connections_scroll: UniformListScrollHandle::new(),
            prepared,
            prepared_revision: revision,
            initial_frame_pending: true,
            pending_anchor: None,
            pending_preview: None,
            link_style_revision: (u64::MAX, u64::MAX),
            requested_revision: if complete { revision } else { u64::MAX },
            _prepare: None,
            _disk_reload: None,
            prepare_cancelled: Arc::new(AtomicBool::new(false)),
            editor,
            _subscription: subscription,
            _scroll_subscription: scroll_subscription,
            _completion_subscription: completion_subscription,
            _link_subscription: link_subscription,
            _attachment_subscription: attachment_subscription,
        });
    }
}
