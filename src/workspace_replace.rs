struct ReplacePanel {
    root: PathBuf,
    find: Entity<Editor>,
    replacement: Entity<Editor>,
    case_sensitive: bool,
    plans: Arc<Vec<opalglass::changes::NoteChanges>>,
    selected: std::collections::BTreeSet<usize>,
    active: usize,
    preview_key: Option<(String, String, bool)>,
    loading: bool,
    message: Option<String>,
    notes_scroll: UniformListScrollHandle,
    diff_scroll: UniformListScrollHandle,
    cancelled: Arc<AtomicBool>,
    _subscriptions: Vec<Subscription>,
}
impl Drop for ReplacePanel {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}
impl ReplacePanel {
    fn key(&self, cx: &App) -> (String, String, bool) {
        (
            self.find.read(cx).buffer.text().into(),
            self.replacement.read(cx).buffer.text().into(),
            self.case_sensitive,
        )
    }
    fn ready(&self, cx: &App) -> bool {
        !self.loading && self.preview_key.as_ref() == Some(&self.key(cx))
    }
}
impl Workspace {
    fn open_replace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.managing {
            return;
        }
        let Some(vault) = &self.vault else {
            return;
        };
        let root = vault.root.clone();
        let find = cx.new(|cx| Editor::new("", true, "Find text", cx));
        let replacement =
            cx.new(|cx| Editor::new("", true, "Replace with (empty deletes matches)", cx));
        let subscriptions = vec![
            cx.observe(&find, |_, _, cx| cx.notify()),
            cx.observe(&replacement, |_, _, cx| cx.notify()),
        ];
        find.read(cx).focus.focus(window);
        self.replace_panel = Some(ReplacePanel {
            root,
            find,
            replacement,
            case_sensitive: false,
            plans: Arc::new(vec![]),
            selected: Default::default(),
            active: 0,
            preview_key: None,
            loading: false,
            message: None,
            notes_scroll: UniformListScrollHandle::new(),
            diff_scroll: UniformListScrollHandle::new(),
            cancelled: Arc::new(AtomicBool::new(false)),
            _subscriptions: subscriptions,
        });
        cx.notify();
    }
    fn preview_replace(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = &mut self.replace_panel else {
            return;
        };
        let Some(vault) = self.vault.clone().filter(|v| v.root == panel.root) else {
            return;
        };
        panel.cancelled.store(true, Ordering::Relaxed);
        panel.cancelled = Arc::new(AtomicBool::new(false));
        let cancelled = panel.cancelled.clone();
        let key = panel.key(cx);
        panel.loading = true;
        panel.message = None;
        panel.preview_key = None;
        panel.plans = Arc::new(vec![]);
        panel.selected.clear();
        cx.notify();
        self._replace = Some(cx.spawn(async move |this, cx| {
            let work_key = key.clone();
            let flag = cancelled.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    opalglass::changes::replace_preview(
                        &vault,
                        &work_key.0,
                        &work_key.1,
                        work_key.2,
                        &flag,
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(panel) = this
                    .replace_panel
                    .as_mut()
                    .filter(|p| Arc::ptr_eq(&p.cancelled, &cancelled))
                else {
                    return;
                };
                panel.loading = false;
                match result {
                    Ok(plans) => {
                        panel.selected = (0..plans.len()).collect();
                        panel.active = 0;
                        panel.plans = Arc::new(plans);
                        panel.preview_key = Some(key);
                        panel.notes_scroll = UniformListScrollHandle::new();
                        panel.diff_scroll = UniformListScrollHandle::new();
                    }
                    Err(error) => panel.message = Some(error),
                }
                cx.notify();
            });
        }));
    }
    fn apply_replace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.managing || self.checkpointing {
            return;
        }
        let Some(panel) = &self.replace_panel else {
            return;
        };
        if !panel.ready(cx) || panel.selected.is_empty() {
            self.replace_panel.as_mut().unwrap().message = Some(
                "Preview the current find and replacement text, then select notes to apply.".into(),
            );
            cx.notify();
            return;
        }
        let Some(mut vault) = self.vault.clone().filter(|v| v.root == panel.root) else {
            return;
        };
        let selected: Vec<_> = panel
            .selected
            .iter()
            .filter_map(|&i| panel.plans.get(i).cloned())
            .collect();
        if self
            .tabs
            .iter()
            .any(|tab| tab.dirty(cx) && selected.iter().any(|p| p.path == tab.path))
        {
            self.replace_panel.as_mut().unwrap().message = Some("A selected note has unsaved edits. Save it and refresh the preview, or deselect that note.".into());
            cx.notify();
            return;
        }
        self.cancel_indexing();
        self.managing = true;
        self.focus.focus(window);
        self.message("Applying reviewed replacements…", false, cx);
        self._management = Some(cx.spawn_in(window, async move |this, cx| {
            let (vault, applied, error) = cx
                .background_executor()
                .spawn(async move {
                    let (applied, error) = opalglass::changes::apply_batch(&mut vault, &selected);
                    (vault, applied, error)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.managing = false;
                if applied > 0 {
                    this.apply_scan(vault, this.revision, cx);
                }
                let failed = error.is_some();
                let message = match error {
                    Some(error) => format!("Applied to {applied} note(s); stopped: {error}"),
                    None => format!("Replacements applied to {applied} note(s). Undo is available in the command palette."),
                };
                if let Some(panel) = &mut this.replace_panel {
                    panel.preview_key = None;
                    panel.selected.clear();
                    panel.message = Some(message.clone());
                }
                this.message(message, failed, cx);
            });
        }));
    }
    fn render_replace(&self, cx: &mut Context<Self>) -> AnyElement {
        let panel = self.replace_panel.as_ref().unwrap();
        let matches: usize = panel
            .selected
            .iter()
            .filter_map(|&i| panel.plans.get(i))
            .map(|p| p.edits.len())
            .sum();
        let ready = panel.ready(cx);
        let rows = panel.plans.get(panel.active).map(|plan| &plan.preview);
        let count = rows.map_or(0, Vec::len);
        div().absolute().inset_0().occlude().bg(rgba(0x05070bb0)).p_6().flex().items_center().justify_center()
            .child(div().w_full().h_full().max_w(px(1200.)).p_4().rounded_lg().bg(rgb(PANEL)).border_1().border_color(rgb(LINE))
                .flex().flex_col().gap_3()
                .child(div().text_lg().child("Search and replace"))
                .child(caption("Review changes across saved notes. Only selected notes are written."))
                .child(div().flex().gap_2()
                    .child(div().h(px(42.)).flex_1().border_1().border_color(rgb(LINE)).rounded_md().debug_selector(|| "replace-find".into()).child(panel.find.clone()))
                    .child(div().h(px(42.)).flex_1().border_1().border_color(rgb(LINE)).rounded_md().debug_selector(|| "replace-with".into()).child(panel.replacement.clone()))
                    .child(button("replace-case", if panel.case_sensitive { "Aa · Case sensitive" } else { "Aa · Ignore case" }).on_click(cx.listener(|this, _, _, cx| {
                        if let Some(panel) = &mut this.replace_panel { panel.case_sensitive = !panel.case_sensitive; }
                        cx.notify();
                    })))
                    .child(button("replace-preview", "Preview changes").debug_selector(|| "replace-preview".into())
                        .on_click(cx.listener(|this, _, _, cx| this.preview_replace(cx)))))
                .child(caption(if panel.loading { "Building preview…".into() } else if ready { format!("{matches} replacements in {} selected notes", panel.selected.len()) } else { "Preview required before applying changes".into() }))
                .children(panel.plans.get(panel.active).map(|plan| caption(format!("Preview: {} · − original / + replacement", plan.path.display()))))
                .when(ready && panel.plans.is_empty(), |el| el.child(caption("No changes found.")))
                .child(div().flex().flex_1().min_h_0().gap_3()
                    .child(uniform_list("replace-notes", panel.plans.len(), cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        let Some(panel) = &this.replace_panel else { return vec![]; };
                        range.map(|index| {
                            let plan = &panel.plans[index];
                            div().h(px(48.)).flex().items_center().gap_1()
                                .child(button(("replace-select", index), if panel.selected.contains(&index) { "☑" } else { "☐" })
                                    .debug_selector(move || format!("replace-select-{index}"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(panel) = &mut this.replace_panel && !panel.selected.remove(&index) { panel.selected.insert(index); }
                                        cx.notify();
                                    })))
                                .child(button(("replace-note", index), format!("{} · {}", plan.path.display(), plan.edits.len())).flex_1().min_w_0().truncate()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(panel) = &mut this.replace_panel { panel.active = index; panel.diff_scroll = UniformListScrollHandle::new(); }
                                        cx.notify();
                                    })))
                        }).collect::<Vec<_>>()
                    })).track_scroll(panel.notes_scroll.clone()).w(px(260.)).h_full())
                    .child(uniform_list("replace-diff", count, cx.processor(|this, range: std::ops::Range<usize>, _, _| {
                        let Some(plan) = this.replace_panel.as_ref().and_then(|p| p.plans.get(p.active)) else { return vec![]; };
                        range.map(|index| {
                            use opalglass::comparison::Kind;
                            let row = &plan.preview[index];
                            let (marker, color) = match row.kind { Kind::Added => ("+", 0xa9d5c3), Kind::Removed => ("−", 0xf1b0b0), _ => (" ", MUTED) };
                            div().id(("replacement-line", index)).h(px(26.)).text_sm().font_family(MONO_FONT).text_color(rgb(color)).truncate()
                                .child(format!("{marker} {}", row.text)).tooltip({ let text = row.text.clone(); move |_, cx| cx.new(|_| PathTooltip(text.clone())).into() })
                        }).collect::<Vec<_>>()
                    })).track_scroll(panel.diff_scroll.clone()).flex_1().min_w_0().h_full()))
                .children(panel.message.as_ref().map(|message| div().text_sm().child(message.clone())))
                .child(div().flex().justify_end().gap_2()
                    .child(button("replace-close", "Close").on_click(cx.listener(|this, _, window, cx| {
                        this.replace_panel = None; this._replace = None; this.focus_document(window, cx); cx.notify();
                    })))
                    .child(button("replace-apply", format!("Apply to {} notes", panel.selected.len())).debug_selector(|| "replace-apply".into())
                        .when(!ready || panel.selected.is_empty(), |el| el.opacity(0.45))
                        .on_click(cx.listener(|this, _, window, cx| this.apply_replace(window, cx))))))
            .into_any_element()
    }
}
