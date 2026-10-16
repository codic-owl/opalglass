struct ExternalComparison {
    editor: Entity<Editor>,
    revision: u64,
    root: PathBuf,
    path: PathBuf,
    disk: String,
    deleted: bool,
    ours: String,
    rows: Arc<Vec<opalglass::comparison::Row>>,
    disk_changes: Arc<Vec<opalglass::comparison::Row>>,
    show_disk_changes: bool,
    scroll: UniformListScrollHandle,
    error: Option<String>,
}
impl Workspace {
    fn open_comparison(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.managing || self.checkpointing {
            return;
        }
        let (Some(vault), Some(tab)) = (self.vault.clone(), self.tabs.get(self.active)) else {
            return;
        };
        let editor = tab.editor.clone();
        let revision = editor.read(cx).buffer.revision();
        let ours = tab.disk_text(cx);
        let baseline = tab.baseline.clone();
        let path = tab.path.clone();
        let root = vault.root.clone();
        self.focus.focus(window);
        self.message("Comparing versions…", false, cx);
        self._comparison = Some(cx.spawn(async move |this, cx| {
            let read_path = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let (disk, deleted) = match vault.read(&read_path) {
                        Ok(text) => (text, false),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            (String::new(), true)
                        }
                        Err(error) => return Err(error),
                    };
                    let rows = Arc::new(opalglass::comparison::compare(&ours, &disk));
                    let disk_changes = Arc::new(opalglass::comparison::compare(&baseline, &disk));
                    Ok::<_, std::io::Error>((disk, deleted, ours, rows, disk_changes))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.vault.as_ref().is_none_or(|v| v.root != root)
                    || this
                        .tabs
                        .get(this.active)
                        .is_none_or(|t| t.editor != editor || t.path != path)
                    || editor.read(cx).buffer.revision() != revision
                {
                    return;
                }
                match result {
                    Ok((disk, deleted, ours, rows, disk_changes)) => {
                        this.comparison = Some(ExternalComparison {
                            editor,
                            revision,
                            root,
                            path,
                            disk,
                            deleted,
                            ours,
                            rows,
                            disk_changes,
                            show_disk_changes: false,
                            scroll: UniformListScrollHandle::new(),
                            error: None,
                        });
                        this.message(
                            "Review the versions before choosing which to keep",
                            false,
                            cx,
                        );
                    }
                    Err(error) => this.message(format!("Could not compare: {error}"), true, cx),
                }
            });
        }));
    }
    fn resolve_comparison(&mut self, use_disk: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.managing || self.checkpointing {
            return;
        }
        let Some(panel) = &self.comparison else {
            return;
        };
        let Some(mut vault) = self.vault.clone().filter(|v| v.root == panel.root) else {
            return;
        };
        if panel.editor.read(cx).buffer.revision() != panel.revision {
            self.comparison.as_mut().unwrap().error = Some(
                "Your edits changed. Refresh the comparison before choosing a version.".into(),
            );
            cx.notify();
            return;
        }
        let editor = panel.editor.clone();
        let path = panel.path.clone();
        let disk = panel.disk.clone();
        let ours = panel.ours.clone();
        let deleted = panel.deleted;
        if deleted && use_disk {
            return;
        }
        self.cancel_indexing();
        self.managing = true;
        self.focus.focus(window);
        self.message("Applying chosen version…", false, cx);
        self._management = Some(cx.spawn_in(window, async move |this, cx| {
            let read_path = path.clone();
            let result = cx.background_executor().spawn(async move {
                if deleted {
                    match vault.read(&read_path) {
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                        _ => return Err("The note exists again or cannot be read. Refresh the comparison first.".to_owned()),
                    }
                    // Refresh removes the stale inventory entry; create then
                    // uses no-clobber publication if another app recreates it.
                    vault.refresh().map_err(|e| e.to_string())?;
                    vault.create(&read_path.to_string_lossy(), &ours).map_err(|e| e.to_string())?;
                    return Ok((vault, ours));
                }
                if vault.read(&read_path).map_err(|e| e.to_string())? != disk {
                    return Err("The disk version changed again. Refresh the comparison to review the latest changes.".to_owned());
                }
                if use_disk { Ok((vault, disk)) }
                else {
                    vault.save(&read_path, &disk, &ours).map_err(|e| e.to_string())?;
                    Ok((vault, ours))
                }
            }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.managing = false;
                match result {
                    Ok((vault, text)) => {
                        if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.editor == editor && tab.path == path) {
                            if use_disk {
                                let normalized = opalglass::document::normalize(&text);
                                editor.update(cx, |editor, cx| {
                                    editor.buffer.replace(0..editor.buffer.text().len(), &normalized);
                                    editor.reveal_cursor(cx);
                                });
                            }
                            tab.baseline_normalized = Arc::new(opalglass::document::normalize(&text));
                            tab.baseline = text;
                            tab.external_changed = false;
                            tab.dirty_cache.set((u64::MAX, false));
                            if let Some(directory) = &this.state_directory {
                                let _ = opalglass::state::Recovery::remove(directory, &vault.root, &path);
                            }
                        }
                        this.vault = Some(vault);
                        this.revision += 1;
                        this.comparison = None;
                        this.focus_document(window, cx);
                        this.message(if use_disk { "Disk version loaded. Undo restores your previous edits." } else { "Your version saved" }, false, cx);
                    }
                    Err(error) => {
                        if let Some(panel) = &mut this.comparison { panel.error = Some(error.clone()); }
                        this.message(error, true, cx);
                    }
                }
                cx.notify();
            });
        }));
    }
    fn render_comparison(&self, cx: &mut Context<Self>) -> AnyElement {
        let panel = self.comparison.as_ref().unwrap();
        let rows = if panel.show_disk_changes {
            panel.disk_changes.clone()
        } else {
            panel.rows.clone()
        };
        let label = if panel.show_disk_changes {
            "− Version when opened     + Current disk version"
        } else {
            "− Your unsaved version     + Current disk version"
        };
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
                    .w_full()
                    .h_full()
                    .max_w(px(1100.))
                    .p_4()
                    .rounded_lg()
                    .bg(rgb(PANEL))
                    .border_1()
                    .border_color(rgb(LINE))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_lg().child("External changes"))
                    .child(caption(panel.path.display().to_string()))
                    .when(panel.deleted, |el| el.child(caption("This note was deleted outside Opalglass. Your open copy can be restored.")))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                button(
                                    "comparison-mode",
                                    if panel.show_disk_changes {
                                        "Compare with your edits"
                                    } else {
                                        "Show disk changes since opening"
                                    },
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        if let Some(panel) = &mut this.comparison {
                                            panel.show_disk_changes = !panel.show_disk_changes;
                                            panel.scroll = UniformListScrollHandle::new();
                                        }
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                button("comparison-refresh", "Refresh comparison")
                                    .debug_selector(|| "comparison-refresh".into())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_comparison(window, cx)
                                    })),
                            ),
                    )
                    .child(caption(label))
                    .when(rows.is_empty(), |el| {
                        el.child(caption("These versions have identical contents."))
                    })
                    .child(
                        uniform_list(
                            "comparison-lines",
                            rows.len(),
                            cx.processor(move |_, range: std::ops::Range<usize>, _, _| {
                                range
                                    .map(|index| {
                                        use opalglass::comparison::Kind;
                                        let row = &rows[index];
                                        let (marker, background, color) = match row.kind {
                                            Kind::Removed => ("−", 0x39282e, 0xf1b0b0),
                                            Kind::Added => ("+", 0x20372f, 0xa9d5c3),
                                            Kind::Context => (" ", BG, TEXT),
                                            Kind::Divider => ("⋯", PANEL, MUTED),
                                        };
                                        div()
                                            .h(px(26.))
                                            .px_2()
                                            .flex()
                                            .gap_2()
                                            .bg(rgb(background))
                                            .text_color(rgb(color))
                                            .text_sm()
                                            .font_family(MONO_FONT)
                                            .child(
                                                div().w(px(48.)).text_color(rgb(MUTED)).child(
                                                    row.old_line
                                                        .map_or(String::new(), |n| n.to_string()),
                                                ),
                                            )
                                            .child(
                                                div().w(px(48.)).text_color(rgb(MUTED)).child(
                                                    row.new_line
                                                        .map_or(String::new(), |n| n.to_string()),
                                                ),
                                            )
                                            .child(marker)
                                            .child(
                                                div()
                                                    .id(("comparison-line", index))
                                                    .flex_1()
                                                    .min_w_0()
                                                    .truncate()
                                                    .child(row.text.clone())
                                                    .tooltip({
                                                        let text = row.text.clone();
                                                        move |_, cx| {
                                                            cx.new(|_| PathTooltip(text.clone()))
                                                                .into()
                                                        }
                                                    }),
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
                            .child(button("comparison-close", "Keep editing").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.comparison = None;
                                    this.focus_document(window, cx);
                                    cx.notify();
                                }),
                            ))
                            .child(
                                button("comparison-use-disk", "Use disk version")
                                    .debug_selector(|| "comparison-use-disk".into())
                                    .when(panel.deleted, |el| el.opacity(0.4))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.resolve_comparison(true, window, cx)
                                    })),
                            )
                            .child(
                                button("comparison-save-ours", if panel.deleted { "Restore my note" } else { "Save my version over disk" })
                                    .debug_selector(|| "comparison-save-ours".into())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.resolve_comparison(false, window, cx)
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }
}
