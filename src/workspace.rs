use crate::editor::Editor;
use crate::theme::{MONO_FONT, UI_FONT};
use gpui::{prelude::*, *};
use opalglass::{
    buffer::Buffer,
    markdown,
    navigation::{self, State as NavigationState, Tree},
    prepared::{Kind, Prepared, PreviewRow},
    vault::Vault,
};
use std::{
    cell::Cell,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const BG: u32 = 0x151820;
const PANEL: u32 = 0x1b1f2a;
const LINE: u32 = 0x2b3140;
const MUTED: u32 = 0x8c95aa;
const TEXT: u32 = 0xdce0eb;
const ACCENT: u32 = 0xb8a5fa;

struct Tab {
    path: PathBuf,
    baseline: String,
    editor: Entity<Editor>,
    baseline_normalized: Arc<String>,
    dirty_cache: Cell<(u64, bool)>,
    prepared: Arc<Prepared>,
    prepared_revision: u64,
    requested_revision: u64,
    _prepare: Option<Task<()>>,
    _disk_reload: Option<Task<()>>,
    prepare_cancelled: Arc<AtomicBool>,
    preview_scroll: ListState,
    _subscription: Subscription,
}
impl Tab {
    fn dirty(&self, cx: &App) -> bool {
        let buffer = &self.editor.read(cx).buffer;
        if self.dirty_cache.get().0 != buffer.revision() {
            self.dirty_cache.set((
                buffer.revision(),
                buffer.text() != self.baseline_normalized.as_str(),
            ));
        }
        self.dirty_cache.get().1
    }
    fn disk_text(&self, cx: &App) -> String {
        opalglass::document::LineEnding::detect(&self.baseline)
            .encode(self.editor.read(cx).buffer.text())
    }
}

impl Drop for Tab {
    fn drop(&mut self) {
        self.prepare_cancelled.store(true, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Modal {
    None,
    New,
    OpenPath,
    Copy,
    Reload,
    NewFolder,
    Rename,
    Move,
}
#[derive(Clone, Copy, PartialEq)]
enum ViewMode {
    Source,
    Split,
    Preview,
}

pub struct Workspace {
    vault: Option<Vault>,
    tabs: Vec<Tab>,
    active: usize,
    focus: FocusHandle,
    search: Entity<Editor>,
    input: Entity<Editor>,
    modal: Modal,
    mode: ViewMode,
    status: String,
    error: bool,
    revision: u64,
    load_generation: u64,
    loading: bool,
    _load: Option<Task<()>>,
    search_query: String,
    search_revision: u64,
    search_results: Vec<usize>,
    searching: bool,
    open_search_result: bool,
    _search_task: Option<Task<()>>,
    search_cancelled: Arc<AtomicBool>,
    notes_scroll: UniformListScrollHandle,
    results_scroll: UniformListScrollHandle,
    tree: Tree,
    navigation: NavigationState,
    tree_revision: u64,
    tree_focus: FocusHandle,
    resizing_sidebar: bool,
    typeahead: String,
    typeahead_at: std::time::Instant,
    state_directory: Option<PathBuf>,
    persisted_navigation: Option<NavigationState>,
    modal_target: Option<PathBuf>,
    managing: bool,
    _management: Option<Task<()>>,
    note_generation: u64,
    _note_load: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
    _watch: Task<()>,
}

fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .text_sm()
        .cursor_pointer()
        .text_color(rgb(TEXT))
        .hover(|s| s.bg(rgb(0x303749)))
        .child(label.into())
}
fn caption(text: impl Into<SharedString>) -> Div {
    div().text_xs().text_color(rgb(MUTED)).child(text.into())
}

impl Workspace {
    pub fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| Editor::new("", true, "Search notes…", cx));
        let input = cx.new(|cx| Editor::new("", true, "", cx));
        let subscriptions = vec![
            cx.observe(&search, |_, _, cx| cx.notify()),
            cx.observe(&input, |_, _, cx| cx.notify()),
        ];
        let watch = cx.spawn(async |this, cx| {
            loop {
                Timer::after(Duration::from_secs(3)).await;
                let Ok(scan) = this.update(cx, |this, cx| {
                    this.persist_navigation(cx);
                    (!this.managing)
                        .then(|| this.vault.as_ref().map(|v| (v.clone(), this.revision)))
                        .flatten()
                }) else {
                    break;
                };
                let Some((mut vault, revision)) = scan else {
                    continue;
                };
                let result = cx
                    .background_executor()
                    .spawn(async move { vault.refresh().map(|changed| changed.then_some(vault)) })
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if this.revision != revision {
                            return;
                        }
                        match result {
                            Ok(Some(vault)) => this.apply_scan(vault, revision, cx),
                            Ok(None) => (),
                            Err(error) => {
                                this.status = format!("Could not refresh vault: {error}");
                                this.error = true;
                                cx.notify();
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut workspace = Self {
            vault: None,
            tabs: Vec::new(),
            active: 0,
            focus: cx.focus_handle(),
            search,
            input,
            modal: Modal::None,
            mode: ViewMode::Split,
            status: "Your notes, on your disk.".into(),
            error: false,
            revision: 0,
            load_generation: 0,
            loading: false,
            _load: None,
            search_query: String::new(),
            search_revision: u64::MAX,
            search_results: Vec::new(),
            searching: false,
            open_search_result: false,
            _search_task: None,
            search_cancelled: Arc::new(AtomicBool::new(false)),
            notes_scroll: UniformListScrollHandle::new(),
            results_scroll: UniformListScrollHandle::new(),
            tree: Tree::default(),
            navigation: NavigationState::default(),
            tree_revision: u64::MAX,
            tree_focus: cx.focus_handle(),
            resizing_sidebar: false,
            typeahead: String::new(),
            typeahead_at: std::time::Instant::now(),
            state_directory: if cfg!(test) {
                None
            } else {
                navigation::state_directory()
            },
            persisted_navigation: None,
            modal_target: None,
            managing: false,
            _management: None,
            note_generation: 0,
            _note_load: None,
            _subscriptions: subscriptions,
            _watch: watch,
        };
        workspace.focus.focus(window);
        if let Some(path) = path {
            workspace.open_vault(path, window, cx);
        }
        workspace
    }
    fn apply_scan(&mut self, vault: Vault, revision: u64, cx: &mut Context<Self>) {
        if self.managing
            || self.revision != revision
            || self.vault.as_ref().is_none_or(|v| v.root != vault.root)
        {
            return;
        }
        let mut reload = Vec::new();
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            match vault.notes.iter().find(|n| n.path == tab.path) {
                Some(note) if note.text != tab.baseline => {
                    if !tab.dirty(cx) {
                        reload.push(index);
                    } else {
                        self.status = "External changes detected. Save a copy or reload before saving this note.".into();
                        self.error = true;
                    }
                }
                None => {
                    self.status =
                        "An open note was removed from disk. Save a copy to preserve its contents."
                            .into();
                    self.error = true;
                }
                _ => (),
            }
        }
        self.vault = Some(vault);
        self.revision += 1;
        for index in reload {
            self.reload_tab(index, cx);
        }
        cx.notify();
    }
    fn message(&mut self, text: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        self.status = text.into();
        self.error = error;
        cx.notify();
    }
    fn open_vault(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if !self.save_all_in(window, cx) {
            return;
        }
        self.load_generation += 1;
        self.note_generation += 1;
        self._note_load = None;
        let generation = self.load_generation;
        self.loading = true;
        self.modal = Modal::None;
        self.focus_document(window, cx);
        self.message("Opening vault…", false, cx);
        let state_directory = self.state_directory.clone();
        self._load = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    Vault::open(path).map(|vault| {
                        let state = state_directory
                            .map(|d| NavigationState::load(&d, &vault.root))
                            .unwrap_or_default();
                        (vault, state)
                    })
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.load_generation != generation {
                    return;
                }
                this.loading = false;
                // The previous vault remains usable while loading. Preserve any
                // edits made since the request, and abort switching on conflict.
                if !this.save_all_in(window, cx) {
                    return;
                }
                this.finish_open(result, window, cx);
            });
        }));
    }
    fn finish_open(
        &mut self,
        result: std::io::Result<(Vault, NavigationState)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok((vault, state)) => {
                self.note_generation += 1;
                self._note_load = None;
                self.revision += 1;
                let first = vault
                    .notes
                    .iter()
                    .find(|n| n.title == "Welcome")
                    .or_else(|| vault.notes.first())
                    .map(|n| n.path.clone());
                self.navigation = state;
                self.persisted_navigation = Some(self.navigation.clone());
                self.tree.rebuild(&vault, &self.navigation.expanded);
                self.tree_revision = self.revision;
                self.vault = Some(vault);
                self.tabs.clear();
                self.active = 0;
                self.notes_scroll = UniformListScrollHandle::new();
                self.notes_scroll
                    .0
                    .borrow()
                    .base_handle
                    .set_offset(point(px(0.), px(self.navigation.scroll_y)));
                self.results_scroll = UniformListScrollHandle::new();
                self.search.update(cx, |editor, cx| editor.set_text("", cx));
                self.modal = Modal::None;
                self.message("Vault opened", false, cx);
                if let Some(path) = first {
                    self.open_note(path, window, cx);
                } else {
                    self.focus.focus(window);
                }
            }
            Err(error) => self.message(format!("Could not open vault: {error}"), true, cx),
        }
    }
    fn persist_navigation(&mut self, cx: &mut Context<Self>) {
        self.navigation.scroll_y = self.notes_scroll.0.borrow().base_handle.offset().y.into();
        if self.persisted_navigation.as_ref() == Some(&self.navigation) {
            return;
        }
        if let (Some(directory), Some(vault)) = (&self.state_directory, &self.vault)
            && let Err(error) = self.navigation.save(directory, &vault.root)
        {
            self.message(format!("Could not save sidebar state: {error}"), true, cx);
            return;
        }
        self.persisted_navigation = Some(self.navigation.clone());
    }
    fn update_tree(&mut self) {
        if self.tree_revision != self.revision {
            if let Some(vault) = &self.vault {
                self.tree.rebuild(vault, &self.navigation.expanded);
            }
            self.tree_revision = self.revision;
        }
    }
    fn toggle_folder(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.navigation.expanded.remove(&path) {
            self.navigation.expanded.insert(path);
        }
        self.tree.flatten(&self.navigation.expanded);
        cx.notify();
    }
    fn reveal_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.tabs.get(self.active).map(|t| t.path.clone()) else {
            return;
        };
        self.navigation.hidden = false;
        self.search.update(cx, |editor, cx| editor.set_text("", cx));
        self.reveal_path(&path);
        self.tree_focus.focus(window);
        cx.notify();
    }
    fn reveal_path(&mut self, path: &std::path::Path) {
        self.navigation.reveal(path);
        self.update_tree();
        self.tree.flatten(&self.navigation.expanded);
        if let Some(index) = self.tree.visible.iter().position(|e| e.path == path) {
            self.notes_scroll
                .scroll_to_item(index, ScrollStrategy::Center);
        }
    }
    fn tree_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.tree_focus.is_focused(window) || !self.search_query.is_empty() {
            return false;
        }
        self.update_tree();
        if self.tree.visible.is_empty() {
            return false;
        }
        let index = self
            .navigation
            .selected
            .as_ref()
            .and_then(|p| self.tree.visible.iter().position(|e| &e.path == p))
            .unwrap_or(0);
        let entry = self.tree.visible[index].clone();
        let mut next = index;
        match event.keystroke.key.as_str() {
            "down" => {
                next = if self.navigation.selected.is_none() {
                    0
                } else {
                    (index + 1).min(self.tree.visible.len() - 1)
                }
            }
            "up" => next = index.saturating_sub(1),
            "home" => next = 0,
            "end" => next = self.tree.visible.len() - 1,
            "right" if entry.folder => {
                if self.navigation.expanded.contains(&entry.path) {
                    if self
                        .tree
                        .visible
                        .get(index + 1)
                        .is_some_and(|e| e.depth > entry.depth)
                    {
                        next += 1;
                    }
                } else {
                    self.toggle_folder(entry.path.clone(), cx);
                }
            }
            "left" => {
                if entry.folder && self.navigation.expanded.contains(&entry.path) {
                    self.toggle_folder(entry.path.clone(), cx);
                } else if let Some(parent) = entry
                    .path
                    .parent()
                    .and_then(|p| self.tree.visible.iter().position(|e| e.path == p))
                {
                    next = parent;
                }
            }
            "enter" | "space" if entry.folder => self.toggle_folder(entry.path.clone(), cx),
            "enter" => {
                self.open_note(entry.path, window, cx);
                return true;
            }
            _ => {
                let Some(text) = event
                    .keystroke
                    .key_char
                    .as_deref()
                    .filter(|s| !s.chars().any(char::is_control))
                else {
                    return false;
                };
                if self.typeahead_at.elapsed() > Duration::from_millis(800) {
                    self.typeahead.clear();
                }
                self.typeahead_at = std::time::Instant::now();
                self.typeahead.push_str(&text.to_lowercase());
                if let Some(found) = (1..=self.tree.visible.len())
                    .map(|n| (index + n) % self.tree.visible.len())
                    .find(|&i| {
                        self.tree.visible[i]
                            .name()
                            .to_lowercase()
                            .starts_with(&self.typeahead)
                    })
                {
                    next = found;
                }
            }
        }
        self.navigation.selected = Some(self.tree.visible[next].path.clone());
        self.notes_scroll
            .scroll_to_item(next, ScrollStrategy::Center);
        cx.notify();
        true
    }
    fn update_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.search.read(cx).buffer.text();
        if self.search_revision == self.revision && self.search_query == query {
            return;
        }
        let query_changed = self.search_query != query;
        self.search_query = query.to_owned();
        self.search_revision = self.revision;
        self.open_search_result = false;
        self.search_cancelled.store(true, Ordering::Relaxed);
        self.search_cancelled = Arc::new(AtomicBool::new(false));
        self._search_task = None;
        self.search_results.clear();
        self.searching = false;
        if query_changed {
            self.results_scroll = UniformListScrollHandle::new();
        }
        let Some(vault) = self.vault.clone() else {
            return;
        };
        if self.search_query.is_empty() {
            self.search_results = (0..vault.notes.len()).collect();
            return;
        }
        self.searching = true;
        let query = self.search_query.clone();
        let revision = self.revision;
        let cancelled = self.search_cancelled.clone();
        self._search_task = Some(cx.spawn_in(window, async move |this, cx| {
            // Debounce typing before scanning note bodies. Neither the scan nor
            // Unicode case folding runs on the UI thread.
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let search_query = query.clone();
            let results = cx
                .background_executor()
                .spawn(async move { vault.search_indices_cancellable(&search_query, &cancelled) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.revision != revision || this.search.read(cx).buffer.text() != query {
                    return;
                }
                this.search_results = results;
                this.searching = false;
                if std::mem::take(&mut this.open_search_result)
                    && this.search.read(cx).focus.is_focused(window)
                {
                    this.open_first_search_result(window, cx);
                }
                cx.notify();
            });
        }));
    }
    fn open_first_search_result(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.search_results.first().and_then(|&index| {
            self.vault
                .as_ref()?
                .notes
                .get(index)
                .map(|n| n.path.clone())
        });
        if let Some(path) = path {
            self.open_note(path, window, cx);
        }
    }
    fn pick_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Markdown vault".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = result.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        this.open_vault(path, window, cx);
                    }
                }
                Ok(Ok(None)) => (),
                _ => {
                    this.show_modal(Modal::OpenPath, window, cx);
                    this.message(
                        "The folder picker is unavailable. Enter a vault path below.",
                        true,
                        cx,
                    );
                }
            });
        })
        .detach();
    }
    fn open_note(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_note_at(path, None, window, cx);
    }
    fn open_note_at(
        &mut self,
        path: PathBuf,
        heading: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.note_generation += 1;
        self._note_load = None;
        if let Some(index) = self.tabs.iter().position(|t| t.path == path) {
            self.active = index;
            self.finish_note_navigation(heading, window, cx);
            return;
        }
        let Some(vault) = self.vault.clone() else {
            return;
        };
        let root = vault.root.clone();
        let generation = self.note_generation;
        self.modal = Modal::None;
        self.focus.focus(window);
        self.message("Opening note…", false, cx);
        self._note_load = Some(cx.spawn_in(window, async move |this, cx| {
            let read_path = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let text = vault.read(&read_path)?;
                    let buffer = Buffer::new(opalglass::document::normalize(&text));
                    let prepared = Arc::new(Prepared::new(buffer.text()));
                    Ok::<_, std::io::Error>((text, buffer, prepared))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.note_generation != generation
                    || this.vault.as_ref().is_none_or(|v| v.root != root)
                {
                    return;
                }
                match result {
                    Ok((text, buffer, prepared)) => {
                        let normalized = buffer.snapshot();
                        let revision = buffer.revision();
                        let editor = cx.new(|cx| {
                            let mut editor =
                                Editor::from_buffer(buffer, false, "Start writing…", cx);
                            editor.set_source_links(prepared.links.clone());
                            editor
                        });
                        let subscription = cx.observe(&editor, |_, _, cx| cx.notify());
                        this.tabs.push(Tab {
                            path,
                            baseline: text,
                            baseline_normalized: normalized,
                            dirty_cache: Cell::new((revision, false)),
                            preview_scroll: ListState::new(
                                prepared.rows.len(),
                                ListAlignment::Top,
                                px(100.),
                            ),
                            prepared,
                            prepared_revision: revision,
                            requested_revision: revision,
                            _prepare: None,
                            _disk_reload: None,
                            prepare_cancelled: Arc::new(AtomicBool::new(false)),
                            editor,
                            _subscription: subscription,
                        });
                        this.active = this.tabs.len() - 1;
                        this.message("Note opened", false, cx);
                        this.finish_note_navigation(heading, window, cx);
                    }
                    Err(error) => this.message(format!("Could not open note: {error}"), true, cx),
                }
            });
        }));
    }
    fn finish_note_navigation(
        &mut self,
        heading: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(heading) = heading
            && let Some(tab) = self.tabs.get(self.active)
        {
            if let Some((_, index)) = tab
                .prepared
                .headings
                .iter()
                .find(|(title, _)| markdown::heading_matches(title, &heading))
            {
                tab.preview_scroll.scroll_to(ListOffset {
                    item_ix: *index,
                    offset_in_item: px(0.),
                });
            }
            if let Some(offset) =
                markdown::heading_offset(tab.editor.read(cx).buffer.text(), &heading)
            {
                tab.editor
                    .update(cx, |editor, cx| editor.jump_to(offset, cx));
            }
        }
        self.modal = Modal::None;
        self.focus_document(window, cx);
        cx.notify();
    }
    fn update_document(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let revision = tab.editor.read(cx).buffer.revision();
        if tab.requested_revision == revision {
            return;
        }
        tab.requested_revision = revision;
        tab.prepare_cancelled.store(true, Ordering::Relaxed);
        tab.prepare_cancelled = Arc::new(AtomicBool::new(false));
        let cancelled = tab.prepare_cancelled.clone();
        let text = tab.editor.read(cx).buffer.snapshot();
        let editor_id = tab.editor.entity_id();
        tab._prepare = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(75))
                .await;
            let prepared = cx
                .background_executor()
                .spawn(async move { Prepared::new_cancellable(&text, &cancelled).map(Arc::new) })
                .await;
            let Some(prepared) = prepared else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                let Some(tab) = this
                    .tabs
                    .iter_mut()
                    .find(|t| t.editor.entity_id() == editor_id)
                else {
                    return;
                };
                if tab.editor.read(cx).buffer.revision() != revision {
                    return;
                }
                let offset = tab.preview_scroll.logical_scroll_top();
                tab.preview_scroll
                    .splice(0..tab.preview_scroll.item_count(), prepared.rows.len());
                tab.preview_scroll.scroll_to(offset);
                tab.editor.update(cx, |editor, cx| {
                    editor.set_source_links(prepared.links.clone());
                    cx.notify();
                });
                tab.prepared = prepared;
                tab.prepared_revision = revision;
                cx.notify();
            });
        }));
    }
    fn reload_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(vault) = self.vault.clone() else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        let editor_id = tab.editor.entity_id();
        let revision = tab.editor.read(cx).buffer.revision();
        let path = tab.path.clone();
        tab._disk_reload = Some(cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                let text = vault.read(&path)?;
                let buffer = Buffer::new(opalglass::document::normalize(&text));
                let prepared = Arc::new(Prepared::new(buffer.text()));
                Ok::<_, std::io::Error>((text, buffer, prepared))
            }).await;
            let _ = this.update(cx, |this, cx| {
                let Some(tab) = this.tabs.iter_mut().find(|t| t.editor.entity_id() == editor_id) else { return; };
                if tab.editor.read(cx).buffer.revision() != revision {
                    this.message("The note was edited while reloading. Your edits are preserved; reload again or save a copy.", true, cx);
                    return;
                }
                match result {
                    Ok((text, mut buffer, prepared)) => {
                        let cursor = tab.editor.read(cx).buffer.cursor();
                        buffer.move_to(cursor, false);
                        tab.baseline = text;
                        tab.baseline_normalized = buffer.snapshot();
                        let revision = buffer.revision();
                        tab.prepare_cancelled.store(true, Ordering::Relaxed);
                        tab._prepare = None;
                        tab.prepared_revision = revision;
                        tab.requested_revision = revision;
                        tab.dirty_cache.set((revision, false));
                        let offset = tab.preview_scroll.logical_scroll_top();
                        tab.preview_scroll.splice(0..tab.preview_scroll.item_count(), prepared.rows.len());
                        tab.preview_scroll.scroll_to(offset);
                        tab.editor.update(cx, |editor, cx| {
                            editor.set_buffer(buffer, cx);
                            editor.set_source_links(prepared.links.clone());
                        });
                        tab.prepared = prepared;
                        this.revision += 1;
                        this.message("Reloaded from disk", false, cx);
                    }
                    Err(error) => this.message(format!("Could not reload note: {error}"), true, cx),
                }
            });
        }));
    }
    fn focus_document(&self, window: &mut Window, cx: &App) {
        if self.mode != ViewMode::Preview
            && let Some(tab) = self.tabs.get(self.active)
        {
            tab.editor.read(cx).focus.focus(window);
        } else {
            self.focus.focus(window);
        }
    }
    fn set_mode(&mut self, mode: ViewMode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        self.focus_document(window, cx);
        cx.notify();
    }
    fn save_tab(&mut self, index: usize, cx: &mut Context<Self>) -> bool {
        let Some(tab) = self.tabs.get(index) else {
            return true;
        };
        if !tab.dirty(cx) {
            return true;
        }
        let text = tab.disk_text(cx);
        let Some(vault) = &mut self.vault else {
            return false;
        };
        match vault.save(&tab.path, &tab.baseline, &text) {
            Ok(()) => {
                self.revision += 1;
                self.tabs[index].baseline = text;
                self.tabs[index].baseline_normalized =
                    self.tabs[index].editor.read(cx).buffer.snapshot();
                self.tabs[index]
                    .dirty_cache
                    .set((self.tabs[index].editor.read(cx).buffer.revision(), false));
                self.message("Saved to disk", false, cx);
                true
            }
            Err(error) => {
                self.message(error.to_string(), true, cx);
                false
            }
        }
    }
    pub fn save_all(&mut self, cx: &mut Context<Self>) -> bool {
        for index in 0..self.tabs.len() {
            if !self.save_tab(index, cx) {
                self.active = index;
                cx.notify();
                return false;
            }
        }
        true
    }
    pub fn save_all_in(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.managing {
            self.message("Finishing the move and updating links…", false, cx);
            return false;
        }
        self.persist_navigation(cx);
        if self.save_all(cx) {
            return true;
        }
        self.note_generation += 1;
        self._note_load = None;
        // A failed quit or vault switch must direct subsequent input to the
        // conflicting note, including when another tab or dialog had focus.
        self.dismiss(window, cx);
        false
    }
    fn close_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.is_empty() || !self.save_tab(self.active, cx) {
            return;
        }
        self.note_generation += 1;
        self._note_load = None;
        self.tabs.remove(self.active);
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        self.focus_document(window, cx);
        cx.notify();
    }
    fn selected_folder(&self) -> PathBuf {
        let Some(path) = &self.navigation.selected else {
            return PathBuf::new();
        };
        if self
            .vault
            .as_ref()
            .is_some_and(|v| v.folders.contains(path))
        {
            path.clone()
        } else {
            path.parent().unwrap_or(std::path::Path::new("")).into()
        }
    }
    fn move_entry(
        &mut self,
        from: PathBuf,
        to: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.save_all_in(window, cx) {
            return;
        }
        let Some(mut vault) = self.vault.clone() else {
            return;
        };
        self.managing = true;
        self.load_generation += 1;
        self._load = None;
        self.loading = false;
        self.revision += 1;
        self.note_generation += 1;
        self._note_load = None;
        // Any in-flight reload/analysis is superseded by this operation.
        for tab in &mut self.tabs {
            tab._disk_reload = None;
            tab.prepare_cancelled.store(true, Ordering::Relaxed);
            tab._prepare = None;
            tab.requested_revision = u64::MAX;
        }
        let open: Vec<_> = self
            .tabs
            .iter()
            .map(|t| (t.path.clone(), t.baseline.clone()))
            .collect();
        self.focus.focus(window);
        self.message("Moving and updating wiki links…", false, cx);
        self._management = Some(cx.spawn_in(window, async move |this, cx| {
            let source = from.clone();
            let target = to.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    vault.move_entry(&source, &target)?;
                    let ready: Vec<_> = open
                        .into_iter()
                        .map(|(path, baseline)| {
                            let path = opalglass::vault::moved_path(&path, &source, &target);
                            let replacement = vault
                                .notes
                                .iter()
                                .find(|n| n.path == path)
                                .filter(|n| n.text != baseline)
                                .map(|n| {
                                    let buffer =
                                        Buffer::new(opalglass::document::normalize(&n.text));
                                    let prepared = Arc::new(Prepared::new(buffer.text()));
                                    (n.text.clone(), buffer, prepared)
                                });
                            (path, replacement)
                        })
                        .collect();
                    Ok::<_, std::io::Error>((vault, ready))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.managing = false;
                match result {
                    Ok((vault, ready)) => {
                        for (tab, (path, replacement)) in this.tabs.iter_mut().zip(ready) {
                            tab.path = path;
                            if let Some((text, mut buffer, prepared)) = replacement {
                                buffer.move_to(tab.editor.read(cx).buffer.cursor(), false);
                                tab.baseline = text;
                                tab.baseline_normalized = buffer.snapshot();
                                let revision = buffer.revision();
                                tab.dirty_cache.set((revision, false));
                                tab.prepared_revision = revision;
                                tab.requested_revision = revision;
                                let offset = tab.preview_scroll.logical_scroll_top();
                                tab.preview_scroll.splice(
                                    0..tab.preview_scroll.item_count(),
                                    prepared.rows.len(),
                                );
                                tab.preview_scroll.scroll_to(offset);
                                tab.editor.update(cx, |editor, cx| {
                                    editor.set_buffer(buffer, cx);
                                    editor.set_source_links(prepared.links.clone());
                                });
                                tab.prepared = prepared;
                            }
                        }
                        this.navigation.expanded = this
                            .navigation
                            .expanded
                            .iter()
                            .map(|p| opalglass::vault::moved_path(p, &from, &to))
                            .collect();
                        this.vault = Some(vault);
                        this.revision += 1;
                        this.reveal_path(&to);
                        this.dismiss(window, cx);
                        this.tree_focus.focus(window);
                        this.message("Moved; wiki links and open tabs updated", false, cx);
                    }
                    Err(error) => {
                        this.input.read(cx).focus.focus(window);
                        this.message(format!("Could not move: {error}"), true, cx);
                    }
                }
            });
        }));
    }
    fn show_modal(&mut self, modal: Modal, window: &mut Window, cx: &mut Context<Self>) {
        if self.managing {
            return;
        }
        self.note_generation += 1;
        self._note_load = None;
        if matches!(
            modal,
            Modal::New | Modal::Copy | Modal::NewFolder | Modal::Rename | Modal::Move
        ) && self.vault.is_none()
        {
            self.message("Open a vault first", true, cx);
            return;
        }
        if matches!(modal, Modal::Copy | Modal::Reload) && self.tabs.is_empty() {
            return;
        }
        self.modal_target = self.navigation.selected.clone();
        if matches!(modal, Modal::Rename | Modal::Move) && self.modal_target.is_none() {
            self.message("Select a file or folder first", false, cx);
            return;
        }
        let initial = if matches!(modal, Modal::New | Modal::NewFolder) {
            let folder = self.selected_folder();
            if folder.as_os_str().is_empty() {
                String::new()
            } else {
                format!("{}/", folder.display())
            }
        } else if modal == Modal::Rename {
            self.modal_target
                .as_ref()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        } else if modal == Modal::Move {
            self.modal_target.as_ref().unwrap().display().to_string()
        } else if modal == Modal::Copy {
            format!(
                "{} copy",
                self.tabs[self.active]
                    .path
                    .with_extension("")
                    .to_string_lossy()
                    .replace('\\', "/")
            )
        } else {
            String::new()
        };
        self.input.update(cx, |editor, cx| {
            editor.set_text(&initial, cx);
            if matches!(modal, Modal::New | Modal::NewFolder) {
                editor.buffer.move_to(initial.len(), false);
            } else {
                editor.buffer.select(0, initial.len());
            }
        });
        self.modal = modal;
        self.input.read(cx).focus.focus(window);
        cx.notify();
    }
    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.managing {
            return;
        }
        self.modal = Modal::None;
        self.focus_document(window, cx);
        cx.notify();
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.managing {
            return;
        }
        let input = self.input.read(cx).buffer.text().trim().to_owned();
        match self.modal {
            Modal::OpenPath => self.open_vault(PathBuf::from(input), window, cx),
            Modal::New | Modal::Copy => {
                let text = if self.modal == Modal::Copy {
                    let Some(tab) = self.tabs.get(self.active) else {
                        self.dismiss(window, cx);
                        return;
                    };
                    tab.disk_text(cx)
                } else {
                    String::new()
                };
                if let Some(vault) = &mut self.vault {
                    match vault.create(&input, &text) {
                        Ok(path) => {
                            self.revision += 1;
                            self.reveal_path(&path);
                            self.message("Note created", false, cx);
                            self.open_note(path, window, cx);
                        }
                        Err(error) => {
                            self.message(format!("Could not create note: {error}"), true, cx)
                        }
                    }
                }
            }
            Modal::NewFolder => {
                if let Some(vault) = &mut self.vault {
                    match vault.create_folder(&input) {
                        Ok(path) => {
                            self.revision += 1;
                            self.navigation.expanded.insert(path.clone());
                            self.reveal_path(&path);
                            self.dismiss(window, cx);
                            self.tree_focus.focus(window);
                            self.message("Folder created", false, cx);
                        }
                        Err(error) => {
                            self.message(format!("Could not create folder: {error}"), true, cx)
                        }
                    }
                }
            }
            Modal::Rename | Modal::Move => {
                let Some(from) = self.modal_target.clone() else {
                    return;
                };
                let is_folder = self
                    .vault
                    .as_ref()
                    .is_some_and(|v| v.folders.contains(&from));
                let path = if is_folder {
                    Vault::folder_path(&input)
                } else {
                    Vault::note_path(&input)
                };
                let to = match path {
                    Ok(path) => path,
                    Err(error) => {
                        self.message(error.to_string(), true, cx);
                        return;
                    }
                };
                if self.modal == Modal::Rename && to.components().count() != 1 {
                    self.message("Enter a single name; use Move to change folders", true, cx);
                    return;
                }
                let to = if self.modal == Modal::Rename {
                    from.parent().unwrap_or(std::path::Path::new("")).join(to)
                } else {
                    to
                };
                self.move_entry(from, to, window, cx);
            }
            Modal::Reload => {
                self.reload_tab(self.active, cx);
                self.dismiss(window, cx);
            }
            Modal::None => (),
        }
    }
    fn follow(&mut self, target: String, window: &mut Window, cx: &mut Context<Self>) {
        if let (Some(vault), Some(tab)) = (&self.vault, self.tabs.get(self.active)) {
            if let Some(path) = vault.resolve(&tab.path, &target) {
                self.open_note_at(
                    path,
                    target.split_once('#').map(|(_, h)| h.to_owned()),
                    window,
                    cx,
                );
            } else {
                self.show_modal(Modal::New, window, cx);
                self.input.update(cx, |editor, cx| {
                    editor.set_text(target.split('#').next().unwrap_or(&target), cx)
                });
                self.message(
                    "Link target is missing or ambiguous. Choose a path for the new note.",
                    false,
                    cx,
                );
            }
        }
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.managing {
            cx.stop_propagation();
            return;
        }
        let command = (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
            && !event.keystroke.modifiers.alt;
        let shift = event.keystroke.modifiers.shift;
        let key = event.keystroke.key.as_str();
        if self.modal != Modal::None {
            match (command, key) {
                (false, "escape") => self.dismiss(window, cx),
                (false, "enter") => self.submit(window, cx),
                (true, _) => (),
                _ => return,
            }
            // Editing shortcuts were already handled by the dialog's input.
            // Keep workspace commands from mutating the document behind it.
            cx.stop_propagation();
            return;
        }
        if !command && key == "f2" && self.tree_focus.is_focused(window) {
            self.show_modal(Modal::Rename, window, cx);
            cx.stop_propagation();
            return;
        }
        if !command && self.tree_key(event, window, cx) {
            cx.stop_propagation();
            return;
        }
        if command {
            match key {
                "b" if shift => self.reveal_active(window, cx),
                "b" => {
                    self.navigation.hidden = !self.navigation.hidden;
                    self.focus_document(window, cx);
                    cx.notify();
                }
                "s" if shift => self.show_modal(Modal::Copy, window, cx),
                "s" => {
                    self.save_tab(self.active, cx);
                }
                "n" if shift => self.show_modal(Modal::NewFolder, window, cx),
                "n" => self.show_modal(Modal::New, window, cx),
                "o" => self.show_modal(Modal::OpenPath, window, cx),
                "p" | "f" => {
                    self.navigation.hidden = false;
                    self.search.update(cx, |editor, cx| {
                        editor.buffer.select(0, editor.buffer.text().len());
                        cx.notify();
                    });
                    self.search.read(cx).focus.focus(window);
                }
                "w" => self.close_tab(window, cx),
                "e" => {
                    let mode = match self.mode {
                        ViewMode::Source => ViewMode::Split,
                        ViewMode::Split => ViewMode::Preview,
                        ViewMode::Preview => ViewMode::Source,
                    };
                    self.set_mode(mode, window, cx);
                }
                "q" => {
                    if self.save_all_in(window, cx) {
                        cx.quit();
                    }
                }
                _ => return,
            }
        } else if key == "escape" {
            self.dismiss(window, cx);
        } else if key == "enter" && self.search.read(cx).focus.is_focused(window) {
            self.update_search(window, cx);
            if self.searching {
                self.open_search_result = true;
            } else {
                self.open_first_search_result(window, cx);
            }
        } else {
            return;
        }
        cx.stop_propagation();
    }
    fn preview(&self, cx: &mut Context<Self>) -> AnyElement {
        let tab = &self.tabs[self.active];
        let prepared = tab.prepared.clone();
        if prepared.rows.is_empty() {
            return div()
                .flex_1()
                .h_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .child(
                    div()
                        .text_lg()
                        .text_color(rgb(TEXT))
                        .child("Your note starts here"),
                )
                .child(caption("Switch to Source to start writing."))
                .child(
                    button("empty-source", "Start writing")
                        .debug_selector(|| "empty-source".into())
                        .bg(rgb(0x343049))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.set_mode(ViewMode::Source, window, cx)
                        })),
                )
                .into_any_element();
        }
        list(
            tab.preview_scroll.clone(),
            cx.processor(move |this, index, _, cx| {
                div()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(760.))
                            .debug_selector(move || format!("preview-column-{index}"))
                            .min_w_0()
                            .child(this.preview_row(index, &prepared.rows[index], cx)),
                    )
                    .into_any_element()
            }),
        )
        .flex_1()
        .min_w_0()
        .h_full()
        .px_6()
        .py_8()
        .bg(rgb(0x181c25))
        .text_size(px(16.))
        .line_height(relative(1.7))
        .into_any_element()
    }
    fn preview_row(&self, index: usize, row: &PreviewRow, cx: &mut Context<Self>) -> AnyElement {
        let mut style = match row.kind {
            Kind::Heading(level) => div()
                .line_height(relative(1.3))
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(match level {
                    1 => 32.,
                    2 => 25.,
                    3 => 21.,
                    _ => 18.,
                }))
                .text_color(rgb(0xf0edf9)),
            Kind::Paragraph => div(),
            Kind::Quote => div()
                .border_l_2()
                .border_color(rgb(ACCENT))
                .px_4()
                .py_2()
                .bg(rgb(0x202532))
                .text_color(rgb(0xb7c1d4)),
            Kind::Item => div().pl_2(),
            Kind::Task(done) => div()
                .pl_2()
                .text_color(rgb(if done { MUTED } else { TEXT })),
            Kind::Code => {
                let code = row.code.clone().unwrap_or_else(|| row.text.clone());
                return div()
                    .when(!row.continuation, |el| {
                        el.mt_4().child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .px_4()
                                .py_1()
                                .border_b_1()
                                .border_color(rgb(LINE))
                                .child(caption("CODE"))
                                .child(
                                    button(("copy-code", index), "Copy code")
                                        .debug_selector(move || format!("copy-code-{index}"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                code.to_string(),
                                            ));
                                            this.message("Code copied to clipboard", false, cx);
                                        })),
                                ),
                        )
                    })
                    .bg(rgb(0x10131a))
                    .child(
                        div()
                            .px_4()
                            .py_2()
                            .font_family(MONO_FONT)
                            .text_size(px(14.))
                            .line_height(relative(1.6))
                            .text_color(rgb(0xc2d8ce))
                            .child(SharedString::from(row.text.clone())),
                    )
                    .into_any_element();
            }
            Kind::Rule => return div().h(px(1.)).my_3().bg(rgb(LINE)).into_any_element(),
        };
        if !row.continuation {
            style = style.mt(px(match row.kind {
                Kind::Heading(_) if index > 0 => 28.,
                Kind::Heading(_) => 0.,
                Kind::Item | Kind::Task(_) => 6.,
                _ => 16.,
            }));
        }
        let spans = &row.spans;
        let highlights = spans
            .iter()
            .map(|span| {
                let s = &span.style;
                (
                    span.range.clone(),
                    HighlightStyle {
                        color: if s.target.is_some() {
                            Some(rgb(ACCENT).into())
                        } else if s.code {
                            Some(rgb(0xa9d5c3).into())
                        } else {
                            None
                        },
                        underline: s.target.as_ref().map(|_| UnderlineStyle {
                            thickness: px(1.),
                            color: Some(rgb(0x786caa).into()),
                            wavy: false,
                        }),
                        font_weight: s.bold.then_some(FontWeight::BOLD),
                        font_style: s.italic.then_some(FontStyle::Italic),
                        background_color: s.code.then(|| rgb(0x252e37).into()),
                        strikethrough: s.strike.then_some(StrikethroughStyle {
                            thickness: px(1.),
                            color: None,
                        }),
                        ..Default::default()
                    },
                )
            })
            .collect::<Vec<_>>();
        let ranges = spans
            .iter()
            .filter(|s| s.style.target.is_some())
            .map(|s| s.range.clone())
            .collect();
        let targets = spans
            .iter()
            .filter_map(|s| s.style.target.clone())
            .collect::<Vec<_>>();
        let entity = cx.entity().downgrade();
        let styled =
            StyledText::new(SharedString::from(row.text.clone())).with_highlights(highlights);
        let interactive = InteractiveText::new(("inline", index), styled).on_click(
            ranges,
            move |index, window, cx| {
                if let Some(target) = targets.get(index) {
                    if target.starts_with("https://")
                        || target.starts_with("http://")
                        || target.starts_with("mailto:")
                    {
                        cx.open_url(target);
                    } else {
                        let _ =
                            entity.update(cx, |this, cx| this.follow(target.clone(), window, cx));
                    }
                }
            },
        );
        style.child(interactive).into_any_element()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.update_search(window, cx);
        self.update_tree();
        if !self.managing {
            self.update_document(cx);
        }
        let vault_name = self
            .vault
            .as_ref()
            .and_then(|v| v.root.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or("No vault open".into());
        let mut sidebar = div()
            .w(px(self.navigation.width))
            .h_full()
            .flex_shrink_0()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(LINE))
            .flex()
            .flex_col()
            .child(
                div()
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(ACCENT))
                            .child("◈  opalglass"),
                    )
                    .child(caption(vault_name)),
            )
            .child(
                div()
                    .mx_3()
                    .h(px(40.))
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(BG))
                    .child(self.search.clone()),
            )
            .child(
                div()
                    .px_4()
                    .pt_5()
                    .pb_2()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(caption(if self.searching {
                        "SEARCHING…".into()
                    } else {
                        format!("NOTES · {}", self.search_results.len())
                    }))
                    .child(button("new", "+ New").on_click(
                        cx.listener(|this, _, window, cx| this.show_modal(Modal::New, window, cx)),
                    )),
            );
        sidebar = sidebar.child(
            div()
                .px_2()
                .pb_2()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .flex_wrap()
                        .child(button("new-folder", "+ Folder").on_click(
                            cx.listener(|this, _, w, cx| this.show_modal(Modal::NewFolder, w, cx)),
                        ))
                        .child(button("rename-entry", "Rename").on_click(
                            cx.listener(|this, _, w, cx| this.show_modal(Modal::Rename, w, cx)),
                        ))
                        .child(button("move-entry", "Move").on_click(
                            cx.listener(|this, _, w, cx| this.show_modal(Modal::Move, w, cx)),
                        )),
                )
                .child(
                    caption(
                        self.navigation
                            .selected
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "Vault root".into()),
                    )
                    .truncate(),
                )
                .child(
                    button("vault-root", "Select vault root").on_click(cx.listener(
                        |this, _, w, cx| {
                            this.navigation.selected = None;
                            this.tree_focus.focus(w);
                            cx.notify();
                        },
                    )),
                ),
        );
        let searching = !self.search_query.is_empty();
        let notes = uniform_list(
            if searching { "search-results" } else { "notes" },
            if searching {
                self.search_results.len()
            } else {
                self.tree.visible.len()
            },
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .filter_map(|index| {
                        let (path, title, folder, depth) = if searching {
                            let note = this
                                .vault
                                .as_ref()?
                                .notes
                                .get(*this.search_results.get(index)?)?;
                            (note.path.clone(), note.title.clone(), false, 0)
                        } else {
                            let entry = this.tree.visible.get(index)?;
                            (entry.path.clone(), entry.name(), entry.folder, entry.depth)
                        };
                        let active = this.tabs.get(this.active).is_some_and(|t| t.path == path);
                        let selected = this.navigation.selected.as_ref() == Some(&path);
                        let icon = if folder {
                            if this.navigation.expanded.contains(&path) {
                                "▾ ▱"
                            } else {
                                "▸ ▱"
                            }
                        } else {
                            "  ▤"
                        };
                        let tooltip = this.vault.as_ref()?.root.join(&path).display().to_string();
                        Some(
                            div()
                                .id(("note", index))
                                .h(px(if searching { 56. } else { 32. }))
                                .pl(px(8. + depth as f32 * 16.))
                                .pr_2()
                                .py_1()
                                .rounded_md()
                                .overflow_hidden()
                                .cursor_pointer()
                                .bg(rgb(if active {
                                    0x303047
                                } else if selected {
                                    0x242c3c
                                } else {
                                    PANEL
                                }))
                                .hover(|s| s.bg(rgb(0x292e3e)))
                                .child(
                                    div()
                                        .text_sm()
                                        .truncate()
                                        .text_color(rgb(if active { 0xd5c7ff } else { TEXT }))
                                        .child(format!("{icon}  {title}")),
                                )
                                .when(searching, |el| {
                                    el.child(caption(path.display().to_string()).truncate())
                                })
                                .tooltip(move |_, cx| {
                                    cx.new(|_| PathTooltip(tooltip.clone())).into()
                                })
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener({
                                        let path = path.clone();
                                        move |this, _, window, cx| {
                                            this.navigation.selected = Some(path.clone());
                                            this.tree_focus.focus(window);
                                            cx.notify();
                                        }
                                    }),
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.navigation.selected = Some(path.clone());
                                    if folder {
                                        this.tree_focus.focus(window);
                                        this.toggle_folder(path.clone(), cx);
                                    } else {
                                        this.open_note(path.clone(), window, cx);
                                    }
                                })),
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(if searching {
            self.results_scroll.clone()
        } else {
            self.notes_scroll.clone()
        })
        .track_focus(&self.tree_focus)
        .flex_1()
        .min_h_0()
        .px_2();
        sidebar = sidebar.child(notes).child(
            div()
                .p_3()
                .border_t_1()
                .border_color(rgb(LINE))
                .flex()
                .flex_col()
                .gap_1()
                .child(button("open-vault", "Open vault…   Ctrl O").on_click(
                    cx.listener(|this, _, window, cx| this.show_modal(Modal::OpenPath, window, cx)),
                ))
                .child(
                    button("browse-vault", "Browse folders…")
                        .on_click(cx.listener(|this, _, window, cx| this.pick_vault(window, cx))),
                )
                .child(caption("LOCAL FILES · NO ACCOUNT")),
        );
        let mut main = div().flex_1().min_w_0().h_full().flex().flex_col();
        main =
            main.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        button("toggle-sidebar", "☰").on_click(cx.listener(|this, _, _, cx| {
                            this.navigation.hidden = !this.navigation.hidden;
                            cx.notify();
                        })),
                    )
                    .child(button("reveal-active", "Reveal active note").on_click(
                        cx.listener(|this, _, window, cx| this.reveal_active(window, cx)),
                    )),
            );
        let mut tabs = div()
            .id("tabs")
            .overflow_x_scroll()
            .h(px(43.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .bg(rgb(0x181c25))
            .border_b_1()
            .border_color(rgb(LINE));
        for (index, tab) in self.tabs.iter().enumerate() {
            let title = tab.path.file_stem().unwrap_or_default().to_string_lossy();
            tabs = tabs.child(
                div()
                    .id(("tab", index))
                    .h_full()
                    .px_4()
                    .flex()
                    .items_center()
                    .border_r_1()
                    .border_color(rgb(LINE))
                    .cursor_pointer()
                    .bg(rgb(if index == self.active { BG } else { 0x181c25 }))
                    .text_sm()
                    .text_color(rgb(if index == self.active { TEXT } else { MUTED }))
                    .child(format!(
                        "{}{}",
                        title,
                        if tab.dirty(cx) { "  •" } else { "" }
                    ))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.note_generation += 1;
                        this._note_load = None;
                        this.active = index;
                        this.focus_document(window, cx);
                        cx.notify();
                    })),
            );
        }
        main = main.child(tabs);
        if let Some(tab) = self.tabs.get(self.active) {
            let prepared = tab.prepared.clone();
            let editor = tab.editor.clone();
            let path = tab.path.clone();
            let mut tools = div()
                .px_4()
                .py_2()
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(rgb(LINE))
                .child(caption(path.display().to_string()));
            let mut modes = div().flex().gap_1();
            for (id, label, mode) in [
                ("source", "Source", ViewMode::Source),
                ("split", "Split", ViewMode::Split),
                ("reading", "Reading", ViewMode::Preview),
            ] {
                modes = modes.child(
                    button(id, label)
                        .when(self.mode == mode, |s| {
                            s.bg(rgb(0x343049)).text_color(rgb(ACCENT))
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.set_mode(mode, window, cx);
                        })),
                );
            }
            tools =
                tools.child(
                    modes
                        .child(div().w(px(8.)))
                        .child(
                            button("save", "Save").on_click(cx.listener(|this, _, _, cx| {
                                this.save_tab(this.active, cx);
                            })),
                        )
                        .child(button("copy", "Save copy").on_click(
                            cx.listener(|this, _, w, cx| this.show_modal(Modal::Copy, w, cx)),
                        ))
                        .child(button("reload", "Reload").on_click(
                            cx.listener(|this, _, w, cx| this.show_modal(Modal::Reload, w, cx)),
                        ))
                        .child(button("close", "×").on_click(
                            cx.listener(|this, _, window, cx| this.close_tab(window, cx)),
                        )),
                );
            main = main.child(tools);
            let mut panes = div().flex_1().min_h_0().flex();
            if self.mode != ViewMode::Preview {
                panes = panes.child(div().flex_1().min_w_0().h_full().child(editor));
            }
            if self.mode == ViewMode::Split {
                panes = panes.child(div().w(px(1.)).h_full().bg(rgb(LINE)));
            }
            if self.mode != ViewMode::Source {
                panes = panes.child(self.preview(cx));
            }
            main = main.child(panes);
            let mut links = div()
                .px_5()
                .py_3()
                .border_t_1()
                .border_color(rgb(LINE))
                .flex()
                .items_center()
                .gap_3()
                .flex_wrap()
                .child(caption("BACKLINKS"));
            let backlinks = self
                .vault
                .as_ref()
                .map(|v| {
                    v.backlinks(&path)
                        .into_iter()
                        .map(|n| (n.path.clone(), n.title.clone()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if backlinks.is_empty() {
                links = links.child(caption("No incoming links yet"));
            }
            for (index, (path, title)) in backlinks.into_iter().enumerate() {
                links = links.child(
                    button(("backlink", index), format!("↗ {title}"))
                        .text_color(rgb(ACCENT))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_note(path.clone(), window, cx)
                        })),
                );
            }
            let tags = prepared.tags.clone();
            if !tags.is_empty() {
                links = links.child(caption("TAGS"));
            }
            for (index, tag) in tags.into_iter().enumerate() {
                links = links.child(
                    button(("tag", index), format!("#{tag}"))
                        .text_color(rgb(0x9bbfb3))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.search
                                .update(cx, |editor, cx| editor.set_text(&format!("#{tag}"), cx));
                            this.search.read(cx).focus.focus(window);
                        })),
                );
            }
            main = main.child(
                div()
                    .id("connections")
                    .max_h(px(130.))
                    .overflow_y_scroll()
                    .child(links),
            );
        } else {
            main = main.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .items_center()
                    .gap_4()
                    .child(div().text_size(px(52.)).text_color(rgb(ACCENT)).child("◈"))
                    .child(
                        div()
                            .text_3xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("A little space to think."),
                    )
                    .child(
                        div()
                            .text_color(rgb(MUTED))
                            .child("Plain Markdown. Connected ideas. Entirely yours."),
                    )
                    .child(
                        button("welcome-open", "Open a vault folder")
                            .bg(rgb(0x343049))
                            .text_color(rgb(ACCENT))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show_modal(Modal::OpenPath, window, cx)
                            })),
                    )
                    .child(caption(
                        "Ctrl N  new note     ·     Ctrl P  find a note     ·     Ctrl S  save",
                    )),
            );
        }
        let metrics = self
            .tabs
            .get(self.active)
            .map(|tab| {
                let buffer = &tab.editor.read(cx).buffer;
                let (line, column) = buffer.line_column();
                format!(
                    "{} words   ·   Ln {line}, Col {column}   ·   UTF-8",
                    tab.prepared.words
                )
            })
            .unwrap_or_default();
        let mut root = div()
            .size_full()
            .relative()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .font_family(UI_FONT)
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.resizing_sidebar && event.pressed_button == Some(MouseButton::Left) {
                    this.navigation.width = f32::from(event.position.x).clamp(180., 600.);
                    cx.notify();
                } else {
                    this.resizing_sidebar = false;
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.resizing_sidebar = false),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .when(!self.navigation.hidden, |el| {
                        el.child(sidebar).child(
                            div()
                                .id("sidebar-resize")
                                .w(px(5.))
                                .h_full()
                                .flex_shrink_0()
                                .cursor_col_resize()
                                .hover(|s| s.bg(rgb(ACCENT)))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| {
                                        this.resizing_sidebar = true;
                                        cx.stop_propagation();
                                    }),
                                ),
                        )
                    })
                    .child(main),
            )
            .child(
                div()
                    .px_4()
                    .py_2()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(rgb(LINE))
                    .flex()
                    .justify_between()
                    .text_xs()
                    .child(
                        div()
                            .text_color(rgb(if self.error { 0xf1b18e } else { MUTED }))
                            .child(self.status.clone()),
                    )
                    .child(caption(metrics)),
            );
        if self.modal != Modal::None && !self.managing {
            let (title, hint, action) = match self.modal {
                Modal::New => (
                    "New note",
                    "A name or folder/name. The .md extension is added for you.",
                    "Create note",
                ),
                Modal::Copy => (
                    "Save a copy",
                    "Preserve your current edits in a separate Markdown file.",
                    "Save copy",
                ),
                Modal::OpenPath => (
                    "Open vault",
                    "Enter the full path to an existing folder of Markdown notes.",
                    "Open folder",
                ),
                Modal::Reload => (
                    "Reload from disk?",
                    "This discards unsaved edits in the current tab. Save a copy first to keep them.",
                    "Discard edits and reload",
                ),
                Modal::NewFolder => (
                    "New folder",
                    "Enter a folder path. Its parent must already exist.",
                    "Create folder",
                ),
                Modal::Rename => (
                    "Rename",
                    "A new name. Saves open edits and updates resolved wiki links.",
                    "Rename",
                ),
                Modal::Move => (
                    "Move",
                    "Full destination path from the vault root, including the file or folder name. Parent must exist. Saves edits and updates wiki links.",
                    "Move",
                ),
                Modal::None => unreachable!(),
            };
            root = root.child(
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
                            .p_6()
                            .rounded_lg()
                            .border_1()
                            .border_color(rgb(0x49435f))
                            .bg(rgb(PANEL))
                            .shadow_lg()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .child(caption(hint))
                            .when(self.modal != Modal::Reload, |el| {
                                el.child(
                                    div()
                                        .h(px(44.))
                                        .border_1()
                                        .border_color(rgb(ACCENT))
                                        .rounded_md()
                                        .bg(rgb(BG))
                                        .child(self.input.clone()),
                                )
                            })
                            .when(self.error, |el| {
                                el.child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(0xf1b18e))
                                        .child(self.status.clone()),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .gap_3()
                                    .child(button("cancel", "Cancel").on_click(
                                        cx.listener(|this, _, w, cx| this.dismiss(w, cx)),
                                    ))
                                    .child(
                                        button("confirm", action)
                                            .bg(rgb(0x403758))
                                            .text_color(rgb(0xd8ccff))
                                            .on_click(
                                                cx.listener(|this, _, w, cx| this.submit(w, cx)),
                                            ),
                                    ),
                            ),
                    ),
            );
        }
        if self.managing {
            root = root.child(
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
                            .p_6()
                            .rounded_lg()
                            .bg(rgb(PANEL))
                            .child("Moving and updating wiki links…"),
                    ),
            );
        }
        root
    }
}

struct PathTooltip(String);
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
