use crate::editor::Editor;
use crate::theme::{MISSING_LINK, MONO_FONT, UI_FONT, missing_link_underline};
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
    time::{Duration, Instant},
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
    initial_frame_pending: bool,
    pending_heading: Option<String>,
    link_style_revision: (u64, u64),
    requested_revision: u64,
    _prepare: Option<Task<()>>,
    _disk_reload: Option<Task<()>>,
    prepare_cancelled: Arc<AtomicBool>,
    preview_scroll: ListState,
    connections_cache: Option<ConnectionSnapshot>,
    connections_scroll: UniformListScrollHandle,
    _subscription: Subscription,
}
impl Tab {
    fn cancel_opening_analysis(&mut self) {
        if !self.prepared.complete {
            self.prepare_cancelled.store(true, Ordering::Relaxed);
            self._prepare = None;
            self.requested_revision = u64::MAX;
        }
    }
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

struct ExplorerMenu {
    target: Option<PathBuf>,
    position: Point<Pixels>,
    selected: usize,
}

#[derive(Clone)]
struct EntryDrag {
    root: PathBuf,
    path: PathBuf,
}

#[derive(Clone)]
struct TabDrag {
    editor: EntityId,
    title: String,
}

pub struct Workspace {
    vault: Option<Vault>,
    tabs: Vec<Tab>,
    active: usize,
    focus: FocusHandle,
    search: Entity<Editor>,
    input: Entity<Editor>,
    modal: Modal,
    inline_parent: Option<PathBuf>,
    explorer_menu: Option<ExplorerMenu>,
    move_candidates: Vec<PathBuf>,
    move_results: Vec<PathBuf>,
    move_query: String,
    move_selected: usize,
    move_scroll: UniformListScrollHandle,
    tab_drop: Option<usize>,
    tabs_scroll: ScrollHandle,
    mode: ViewMode,
    sidebar: Entity<Sidebar>,
    preview_pane: Entity<PreviewPane>,
    preview_revision: Option<(EntityId, u64, u64, bool)>,
    connections: Entity<Connections>,
    connections_revision: Option<(EntityId, u64, u64, bool, bool)>,
    performance: Entity<PerformanceBar>,
    status: String,
    error: bool,
    revision: u64,
    load_generation: u64,
    loading: bool,
    _load: Option<Task<()>>,
    load_cancelled: Arc<AtomicBool>,
    indexing: bool,
    index_revision: u64,
    index_cancelled: Arc<AtomicBool>,
    _index: Option<Task<()>>,
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
    note_loading: bool,
    _subscriptions: Vec<Subscription>,
    _watch: Task<()>,
}

impl Drop for Workspace {
    fn drop(&mut self) {
        self.load_cancelled.store(true, Ordering::Relaxed);
        self.index_cancelled.store(true, Ordering::Relaxed);
        self.search_cancelled.store(true, Ordering::Relaxed);
    }
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

fn external_link(target: &str) -> bool {
    target.starts_with("https://") || target.starts_with("http://") || target.starts_with("mailto:")
}

impl Workspace {
    pub fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| Editor::new("", true, "Search notes…", cx));
        let input = cx.new(|cx| Editor::new("", true, "", cx));
        let owner = cx.entity().downgrade();
        let sidebar = cx.new(|_| Sidebar {
            #[cfg(test)]
            render_count: 0,
            owner: owner.clone(),
        });
        let preview_pane = cx.new(|_| PreviewPane {
            #[cfg(test)]
            render_count: 0,
            owner: owner.clone(),
        });
        let connections = cx.new(|_| Connections {
            #[cfg(test)]
            render_count: 0,
            owner,
            items: Arc::new(Vec::new()),
            backlinks: 0,
            tags: 0,
            empty_message: "No incoming links yet",
            scroll: UniformListScrollHandle::new(),
        });
        let subscriptions = vec![
            cx.observe(&search, |_, _, cx| cx.notify()),
            cx.observe(&input, |_, _, cx| cx.notify()),
            cx.observe(&cx.entity(), |this, _, cx| {
                // Invalidate cached children before GPUI starts the next frame.
                this.update_indexing(cx);
                this.update_link_styles(cx);
                this.update_panes(cx);
                this.sidebar.update(cx, |_, cx| cx.notify());
            }),
        ];
        let watch = cx.spawn(async |this, cx| {
            loop {
                Timer::after(Duration::from_secs(3)).await;
                let Ok(scan) = this.update(cx, |this, cx| {
                    this.persist_navigation(cx);
                    (!this.managing && !this.loading && !this.indexing)
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
            inline_parent: None,
            explorer_menu: None,
            move_candidates: Vec::new(),
            move_results: Vec::new(),
            move_query: String::new(),
            move_selected: 0,
            move_scroll: UniformListScrollHandle::new(),
            tab_drop: None,
            tabs_scroll: ScrollHandle::new(),
            mode: ViewMode::Split,
            sidebar,
            preview_pane,
            preview_revision: None,
            connections,
            connections_revision: None,
            performance: cx.new(|_| PerformanceBar::default()),
            status: "Your notes, on your disk.".into(),
            error: false,
            revision: 0,
            load_generation: 0,
            loading: false,
            _load: None,
            load_cancelled: Arc::new(AtomicBool::new(false)),
            indexing: false,
            index_revision: u64::MAX,
            index_cancelled: Arc::new(AtomicBool::new(false)),
            _index: None,
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
            note_loading: false,
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
    fn log_performance(
        &self,
        action: &'static str,
        target: String,
        started: Instant,
        cx: &mut Context<Self>,
    ) {
        self.log_performance_phases(
            action,
            target,
            started,
            vec![("Action", started.elapsed())],
            cx,
        );
    }
    fn log_performance_phases(
        &self,
        action: &'static str,
        target: String,
        started: Instant,
        phases: Vec<(&'static str, Duration)>,
        cx: &mut Context<Self>,
    ) {
        self.performance.update(cx, |bar, cx| {
            bar.pending.push(PendingPerformance {
                action,
                target,
                started,
                ready: Instant::now(),
                frame_started: None,
                phases,
            });
            cx.notify();
        });
    }
    fn open_vault(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let started = Instant::now();
        let target = path.display().to_string();
        if !self.save_all_in(window, cx) {
            self.log_performance("Vault switch blocked", target, started, cx);
            return;
        }
        self.cancel_indexing();
        self.load_cancelled.store(true, Ordering::Relaxed);
        self.load_cancelled = Arc::new(AtomicBool::new(false));
        let cancelled = self.load_cancelled.clone();
        self.load_generation += 1;
        self.note_generation += 1;
        self._note_load = None;
        self.note_loading = false;
        let generation = self.load_generation;
        self.loading = true;
        self.modal = Modal::None;
        self.inline_parent = None;
        self.explorer_menu = None;
        self.focus_document(window, cx);
        self.message("Opening vault…", false, cx);
        let state_directory = self.state_directory.clone();
        self._load = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    Vault::discover(path, &cancelled).map(|vault| {
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
                    this.log_performance("Vault switch blocked", target, started, cx);
                    return;
                }
                let action = if result.is_ok() {
                    "Open vault"
                } else {
                    "Vault open failed"
                };
                this.finish_open(result, window, cx);
                this.log_performance(action, target, started, cx);
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
                self.note_loading = false;
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
                self.inline_parent = None;
                self.explorer_menu = None;
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
    fn cancel_indexing(&mut self) {
        self.index_cancelled.store(true, Ordering::Relaxed);
        self._index = None;
        self.indexing = false;
        self.index_revision = u64::MAX;
    }
    fn update_indexing(&mut self, cx: &mut Context<Self>) {
        // Let the first note open before scheduling the bulk index, including
        // on machines with only one background worker.
        if self.loading
            || self.managing
            || (self.note_loading && self.tabs.is_empty())
            || self
                .tabs
                .get(self.active)
                .is_some_and(|tab| tab.initial_frame_pending)
        {
            return;
        }
        let Some(vault) = &self.vault else { return };
        if vault.content_indexed() {
            if self.indexing {
                self.cancel_indexing();
            }
            return;
        }
        if self.index_revision == self.revision {
            return;
        }
        let mut vault = vault.clone();
        self.cancel_indexing();
        self.index_cancelled = Arc::new(AtomicBool::new(false));
        let cancelled = self.index_cancelled.clone();
        let revision = self.revision;
        self.index_revision = revision;
        self.indexing = true;
        self._index = Some(cx.spawn(async move |this, cx| {
            let task_cancelled = cancelled.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    vault.refresh_cancellable(&cancelled)?;
                    Ok::<_, std::io::Error>(vault)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                // A save, create, move or vault switch supersedes this snapshot.
                // The workspace observer restarts indexing from the current vault.
                if task_cancelled.load(Ordering::Relaxed) || this.revision != revision {
                    return;
                }
                this.indexing = false;
                match result {
                    Ok(vault) => this.apply_scan(vault, revision, cx),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => (),
                    Err(error) => this.message(format!("Could not index vault: {error}"), true, cx),
                }
                cx.notify();
            });
        }));
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
        if query_changed {
            self.open_search_result = false;
        }
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
                if this.open_search_result
                    && (!this.search_results.is_empty()
                        || this.vault.as_ref().is_none_or(Vault::content_indexed))
                {
                    this.open_search_result = false;
                    if this.search.read(cx).focus.is_focused(window) {
                        this.open_first_search_result(window, cx);
                    }
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
            if heading.is_none() {
                self.activate_tab(index, window, cx);
            } else {
                self.active = index;
                self.finish_note_navigation(heading, window, cx);
                self.log_performance("Follow heading", path.display().to_string(), started, cx);
            }
            return;
        }
        let Some(vault) = self.vault.clone() else {
            return;
        };
        let root = vault.root.clone();
        let generation = self.note_generation;
        self.modal = Modal::None;
        self.inline_parent = None;
        self.explorer_menu = None;
        self.focus.focus(window);
        self.message("Opening note…", false, cx);
        self.note_loading = true;
        self._note_load = Some(cx.spawn_in(window, async move |this, cx| {
            let read_path = path.clone();
            let (result, mut phases, worker_finished) = cx
                .background_executor()
                .spawn(async move {
                    let work = Instant::now();
                    let text = vault.read(&read_path);
                    let read = Instant::now();
                    let result = text.map(|text| {
                        let buffer = Buffer::new(opalglass::document::normalize(&text));
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
                        let normalized = buffer.snapshot();
                        let revision = buffer.revision();
                        let editor =
                            cx.new(|cx| Editor::from_buffer(buffer, false, "Start writing…", cx));
                        let mut state = (revision, 0, 0);
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
                        let complete = prepared.complete;
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
                            connections_cache: None,
                            connections_scroll: UniformListScrollHandle::new(),
                            prepared,
                            prepared_revision: revision,
                            initial_frame_pending: true,
                            pending_heading: None,
                            link_style_revision: (u64::MAX, u64::MAX),
                            requested_revision: if complete { revision } else { u64::MAX },
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
                phases.push(("UI install", install.elapsed()));
                this.log_performance_phases(action, target, started, phases, cx);
            });
        }));
    }
    fn finish_note_navigation(
        &mut self,
        heading: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.pending_heading = None;
        }
        if let Some(heading) = heading
            && let Some(tab) = self.tabs.get_mut(self.active)
        {
            if !tab.prepared.complete {
                tab.pending_heading = Some(heading.clone());
            }
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
                self.preview_pane.update(cx, |_, cx| cx.notify());
            }
            if let Some(offset) =
                markdown::heading_offset(tab.editor.read(cx).buffer.text(), &heading)
            {
                tab.editor
                    .update(cx, |editor, cx| editor.jump_to(offset, cx));
            }
        }
        self.modal = Modal::None;
        self.inline_parent = None;
        self.explorer_menu = None;
        self.focus_document(window, cx);
        cx.notify();
    }
    fn update_link_styles(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let revision = (tab.prepared_revision, self.revision);
        // Resolve only when the document analysis or vault index changes. Never
        // apply offsets from an older analysis to a buffer with intervening edits.
        if tab.link_style_revision == revision
            || tab.prepared_revision != tab.editor.read(cx).buffer.revision()
        {
            return;
        }
        let Some(vault) = &self.vault else { return };
        let links = tab
            .prepared
            .links
            .iter()
            .map(|link| link.range.clone())
            .collect();
        let missing = tab
            .prepared
            .links
            .iter()
            .filter(|link| {
                !external_link(&link.target) && vault.resolve(&tab.path, &link.target).is_none()
            })
            .map(|link| link.range.clone())
            .collect();
        tab.link_style_revision = revision;
        tab.editor.update(cx, |editor, cx| {
            editor.set_source_links(links, missing);
            cx.notify();
        });
    }
    fn link_highlight(&self, target: &str) -> HighlightStyle {
        let missing = !external_link(target)
            && self.tabs.get(self.active).is_some_and(|tab| {
                self.vault
                    .as_ref()
                    .is_some_and(|vault| vault.resolve(&tab.path, target).is_none())
            });
        HighlightStyle {
            color: Some(rgb(if missing { MISSING_LINK } else { ACCENT }).into()),
            underline: Some(if missing {
                missing_link_underline()
            } else {
                UnderlineStyle {
                    thickness: px(1.),
                    color: Some(rgb(0x786caa).into()),
                    wavy: false,
                }
            }),
            ..Default::default()
        }
    }
    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
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
                    .child(
                        self.folder_drop_target(
                            div()
                                .id("vault-location")
                                .debug_selector(|| "vault-location".into())
                                .py_1()
                                .rounded_md()
                                .cursor_pointer()
                                .text_sm()
                                .text_color(rgb(MUTED))
                                .hover(|s| s.bg(rgb(0x292e3e)))
                                .child(vault_name)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.dismiss(window, cx);
                                    this.navigation.selected = None;
                                    this.tree_focus.focus(window);
                                    cx.notify();
                                }))
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                        this.open_explorer_menu(None, event.position, window, cx);
                                        cx.stop_propagation();
                                    }),
                                ),
                            PathBuf::new(),
                            cx,
                        ),
                    ),
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
                    } else if !self.search_query.is_empty()
                        && self.vault.as_ref().is_some_and(|v| !v.content_indexed())
                    {
                        "RESULTS · indexing contents…".into()
                    } else {
                        format!("NOTES · {}", self.search_results.len())
                    })),
            );
        let searching = !self.search_query.is_empty();
        let inline_index = (!searching).then(|| self.inline_insert_index()).flatten();
        let notes = uniform_list(
            if searching { "search-results" } else { "notes" },
            if searching {
                self.search_results.len()
            } else {
                self.tree.visible.len() + usize::from(inline_index.is_some())
            },
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .filter_map(|index| {
                        if inline_index == Some(index) {
                            return Some(this.inline_row());
                        }
                        let row_index =
                            index - usize::from(inline_index.is_some_and(|i| index > i));
                        let (path, title, folder, depth) = if searching {
                            let note = this
                                .vault
                                .as_ref()?
                                .notes
                                .get(*this.search_results.get(index)?)?;
                            (note.path.clone(), note.title.clone(), false, 0)
                        } else {
                            let entry = this.tree.visible.get(row_index)?;
                            (entry.path.clone(), entry.name(), entry.folder, entry.depth)
                        };
                        if this.inline_parent.is_some()
                            && this.modal == Modal::Rename
                            && this.modal_target.as_ref() == Some(&path)
                        {
                            return Some(this.inline_row());
                        }
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
                                .debug_selector({
                                    let path = path.clone();
                                    move || format!("entry:{}", path.display())
                                })
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
                                        move |this, event: &MouseDownEvent, window, cx| {
                                            this.open_explorer_menu(
                                                Some(path.clone()),
                                                event.position,
                                                window,
                                                cx,
                                            );
                                            cx.stop_propagation();
                                        }
                                    }),
                                )
                                .when(folder, |el| this.folder_drop_target(el, path.clone(), cx))
                                .when(!this.managing, |el| {
                                    el.on_drag(
                                        EntryDrag {
                                            root: this.vault.as_ref().unwrap().root.clone(),
                                            path: path.clone(),
                                        },
                                        |drag, _, _, cx| {
                                            cx.new(|_| PathTooltip(drag.path.display().to_string()))
                                        },
                                    )
                                })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    if this.managing {
                                        return;
                                    }
                                    this.dismiss(window, cx);
                                    this.navigation.selected = Some(path.clone());
                                    if folder {
                                        this.tree_focus.focus(window);
                                        this.toggle_folder(path.clone(), cx);
                                    } else {
                                        this.open_note(path.clone(), window, cx);
                                    }
                                }))
                                .into_any_element(),
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
        sidebar =
            sidebar
                .child(
                    div()
                        .id("explorer")
                        .debug_selector(|| "explorer".into())
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                this.open_explorer_menu(None, event.position, window, cx);
                                cx.stop_propagation();
                            }),
                        )
                        .child(notes),
                )
                .child(
                    div()
                        .p_3()
                        .border_t_1()
                        .border_color(rgb(LINE))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(button("open-vault", "Open vault…   Ctrl O").on_click(
                            cx.listener(|this, _, window, cx| this.pick_vault(window, cx)),
                        )),
                );
        sidebar.into_any_element()
    }
    fn update_panes(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let key = (
            tab.editor.entity_id(),
            tab.prepared_revision,
            self.revision,
            tab.prepared.complete,
        );
        if self.preview_revision != Some(key) {
            self.preview_revision = Some(key);
            self.preview_pane.update(cx, |_, cx| cx.notify());
        }
        let key = (key.0, key.1, key.2, key.3, self.indexing);
        if self.connections_revision == Some(key) {
            return;
        }
        self.connections_revision = Some(key);
        let revision = (key.1, key.2, key.3, key.4);
        let snapshot = match &tab.connections_cache {
            Some(cached) if cached.revision == revision => cached.clone(),
            _ => {
                let backlinks = self
                    .vault
                    .as_ref()
                    .map(|v| v.backlinks(&tab.path))
                    .unwrap_or_default();
                let count = backlinks.len();
                let mut items: Vec<_> = backlinks
                    .into_iter()
                    .map(|note| Connection::Note(note.path.clone(), note.title.clone()))
                    .collect();
                items.extend(tab.prepared.tags.iter().cloned().map(Connection::Tag));
                let message = if self.vault.as_ref().is_some_and(|v| !v.content_indexed()) {
                    if self.indexing {
                        "Indexing backlinks…"
                    } else {
                        "Backlinks not ready"
                    }
                } else {
                    "No incoming links yet"
                };
                let snapshot = ConnectionSnapshot {
                    revision,
                    items: Arc::new(items),
                    backlinks: count,
                    tags: tab.prepared.tags.len(),
                    empty_message: message,
                };
                tab.connections_cache = Some(snapshot.clone());
                snapshot
            }
        };
        let scroll = tab.connections_scroll.clone();
        self.connections.update(cx, |panel, cx| {
            panel.items = snapshot.items;
            panel.backlinks = snapshot.backlinks;
            panel.tags = snapshot.tags;
            panel.empty_message = snapshot.empty_message;
            panel.scroll = scroll;
            cx.notify();
        });
    }
    fn update_document(&mut self, cx: &mut Context<Self>) {
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            if index != self.active {
                tab.cancel_opening_analysis();
            }
        }
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let revision = tab.editor.read(cx).buffer.revision();
        if tab.initial_frame_pending || tab.requested_revision == revision {
            return;
        }
        let initial = !tab.prepared.complete;
        tab.requested_revision = revision;
        tab.prepare_cancelled.store(true, Ordering::Relaxed);
        tab.prepare_cancelled = Arc::new(AtomicBool::new(false));
        let cancelled = tab.prepare_cancelled.clone();
        let text = tab.editor.read(cx).buffer.snapshot();
        let editor_id = tab.editor.entity_id();
        tab._prepare = Some(cx.spawn(async move |this, cx| {
            let task_cancelled = cancelled.clone();
            if !initial {
                cx.background_executor()
                    .timer(Duration::from_millis(75))
                    .await;
            }
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
                if task_cancelled.load(Ordering::Relaxed)
                    || tab.editor.read(cx).buffer.revision() != revision
                {
                    return;
                }
                let offset = tab.preview_scroll.logical_scroll_top();
                tab.preview_scroll
                    .splice(0..tab.preview_scroll.item_count(), prepared.rows.len());
                tab.preview_scroll.scroll_to(offset);
                if let Some(heading) = tab.pending_heading.take()
                    && offset.item_ix == 0
                    && offset.offset_in_item == px(0.)
                    && let Some((_, index)) = prepared
                        .headings
                        .iter()
                        .find(|(title, _)| markdown::heading_matches(title, &heading))
                {
                    tab.preview_scroll.scroll_to(ListOffset {
                        item_ix: *index,
                        offset_in_item: px(0.),
                    });
                }
                tab.prepared = prepared;
                tab.prepared_revision = revision;
                tab.link_style_revision = (u64::MAX, u64::MAX);
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
        self.tabs_scroll.scroll_to_item(self.active);
        if self.mode != ViewMode::Preview
            && let Some(tab) = self.tabs.get(self.active)
        {
            tab.editor.read(cx).focus.focus(window);
        } else {
            self.focus.focus(window);
        }
    }
    fn set_mode(&mut self, mode: ViewMode, window: &mut Window, cx: &mut Context<Self>) {
        let started = Instant::now();
        self.mode = mode;
        self.focus_document(window, cx);
        cx.notify();
        self.log_performance(
            "Change view",
            match mode {
                ViewMode::Source => "Source",
                ViewMode::Split => "Split",
                ViewMode::Preview => "Reading",
            }
            .into(),
            started,
            cx,
        );
    }
    fn save_tab(&mut self, index: usize, cx: &mut Context<Self>) -> bool {
        let started = Instant::now();
        let Some(tab) = self.tabs.get(index) else {
            return true;
        };
        if !tab.dirty(cx) {
            return true;
        }
        let target = tab.path.display().to_string();
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
                self.log_performance("Save note", target, started, cx);
                true
            }
            Err(error) => {
                self.message(error.to_string(), true, cx);
                self.log_performance("Save failed", target, started, cx);
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
        self.note_loading = false;
        // A failed quit or vault switch must direct subsequent input to the
        // conflicting note, including when another tab or dialog had focus.
        self.dismiss(window, cx);
        false
    }
    fn close_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tab_at(self.active, window, cx);
    }
    fn close_tab_at(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let started = Instant::now();
        if self.managing || index >= self.tabs.len() {
            return;
        }
        let target = self.tabs[index].path.display().to_string();
        self.modal = Modal::None;
        self.inline_parent = None;
        self.explorer_menu = None;
        if !self.save_tab(index, cx) {
            self.activate_tab(index, window, cx);
            self.log_performance("Close blocked", target, started, cx);
            return;
        }
        self.note_generation += 1;
        self._note_load = None;
        self.note_loading = false;
        self.tabs.remove(index);
        if index < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        self.focus_document(window, cx);
        cx.notify();
        self.log_performance("Close tab", target, started, cx);
    }
    fn activate_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let started = Instant::now();
        if self.managing || index >= self.tabs.len() {
            return;
        }
        let action = if self.active == index {
            "Tab already open"
        } else {
            "Switch tab"
        };
        let changed = self.active != index
            || self.modal != Modal::None
            || self.inline_parent.is_some()
            || self.explorer_menu.is_some();
        self.note_generation += 1;
        self._note_load = None;
        self.note_loading = false;
        self.active = index;
        self.modal = Modal::None;
        self.inline_parent = None;
        self.explorer_menu = None;
        self.focus_document(window, cx);
        if changed {
            cx.notify();
        }
        self.log_performance(
            action,
            self.tabs[index].path.display().to_string(),
            started,
            cx,
        );
    }
    fn reorder_tab(&mut self, editor: EntityId, before: usize, cx: &mut Context<Self>) {
        let started = Instant::now();
        self.tab_drop = None;
        if self.managing {
            return;
        }
        let Some(from) = self
            .tabs
            .iter()
            .position(|t| t.editor.entity_id() == editor)
        else {
            return;
        };
        let active = self.tabs[self.active].editor.entity_id();
        let tab = self.tabs.remove(from);
        let to = before
            .saturating_sub(usize::from(from < before))
            .min(self.tabs.len());
        self.tabs.insert(to, tab);
        self.tabs_scroll.scroll_to_item(to);
        self.active = self
            .tabs
            .iter()
            .position(|t| t.editor.entity_id() == active)
            .unwrap();
        cx.notify();
        self.log_performance(
            "Reorder tab",
            self.tabs[to].path.display().to_string(),
            started,
            cx,
        );
    }
    fn open_explorer_menu(
        &mut self,
        target: Option<PathBuf>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.managing || self.vault.is_none() {
            return;
        }
        self.dismiss(window, cx);
        self.navigation.selected = target.clone();
        self.explorer_menu = Some(ExplorerMenu {
            target,
            position,
            selected: 0,
        });
        self.tree_focus.focus(window);
        cx.notify();
    }
    fn explorer_actions(&self) -> Vec<(Modal, &'static str, &'static str)> {
        let Some(menu) = &self.explorer_menu else {
            return Vec::new();
        };
        let folder = menu
            .target
            .as_ref()
            .is_none_or(|p| self.vault.as_ref().is_some_and(|v| v.folders.contains(p)));
        let mut actions = Vec::new();
        if folder {
            actions.push((Modal::New, "New note", "Ctrl N"));
            actions.push((Modal::NewFolder, "New folder", "Ctrl Shift N"));
        }
        if menu.target.is_some() {
            actions.push((Modal::Rename, "Rename…", "F2"));
            actions.push((
                Modal::Move,
                if folder {
                    "Move folder to…"
                } else {
                    "Move file to…"
                },
                "",
            ));
        }
        actions
    }
    fn run_explorer_action(&mut self, action: Modal, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.explorer_menu.take() else {
            return;
        };
        self.navigation.selected = menu.target;
        if action == Modal::Move {
            self.show_modal(action, window, cx);
        } else {
            self.show_inline(action, window, cx);
        }
    }
    fn show_inline(&mut self, action: Modal, window: &mut Window, cx: &mut Context<Self>) {
        self.show_modal(action, window, cx);
        if self.modal != action {
            return;
        }
        self.navigation.hidden = false;
        self.search.update(cx, |editor, cx| editor.set_text("", cx));
        let parent = if action == Modal::Rename {
            let target = self.modal_target.clone().unwrap();
            self.reveal_path(&target);
            target
                .parent()
                .unwrap_or(std::path::Path::new(""))
                .to_path_buf()
        } else {
            let parent = self.selected_folder();
            if !parent.as_os_str().is_empty() {
                self.navigation.expanded.insert(parent.clone());
                self.reveal_path(&parent);
                self.tree.flatten(&self.navigation.expanded);
            }
            parent
        };
        let initial = if action == Modal::Rename {
            let path = self.modal_target.as_ref().unwrap();
            if self
                .vault
                .as_ref()
                .is_some_and(|v| v.folders.contains(path))
            {
                path.file_name()
            } else {
                path.file_stem()
            }
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
        } else {
            String::new()
        };
        self.inline_parent = Some(parent);
        self.input.update(cx, |editor, cx| {
            editor.set_text(&initial, cx);
            editor.buffer.select(0, initial.len());
        });
        let index = if action == Modal::Rename {
            self.tree
                .visible
                .iter()
                .position(|e| Some(&e.path) == self.modal_target.as_ref())
                .unwrap_or(0)
        } else {
            self.inline_insert_index().unwrap_or(0)
        };
        self.notes_scroll
            .scroll_to_item(index, ScrollStrategy::Center);
        self.input.read(cx).focus.focus(window);
        cx.notify();
    }
    fn inline_insert_index(&self) -> Option<usize> {
        let parent = self.inline_parent.as_ref()?;
        if !matches!(self.modal, Modal::New | Modal::NewFolder) {
            return None;
        }
        Some(if parent.as_os_str().is_empty() {
            0
        } else {
            self.tree
                .visible
                .iter()
                .position(|e| &e.path == parent)
                .map_or(0, |i| i + 1)
        })
    }
    fn inline_row(&self) -> AnyElement {
        let depth = self
            .inline_parent
            .as_ref()
            .map_or(0, |p| p.components().count());
        div()
            .h(px(32.))
            .pl(px(8. + depth as f32 * 16.))
            .pr_2()
            .child(
                div()
                    .debug_selector(|| "entry-name".into())
                    .h_full()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(if self.error { 0xf1b18e } else { ACCENT }))
                    .bg(rgb(BG))
                    .child(self.input.clone()),
            )
            .into_any_element()
    }
    fn can_move_to(&self, from: &std::path::Path, folder: &std::path::Path) -> bool {
        !self.managing
            && self.vault.as_ref().is_some_and(|v| {
                (folder.as_os_str().is_empty() || v.folders.iter().any(|p| p == folder))
                    && (v.folders.iter().any(|p| p == from)
                        || v.notes.iter().any(|n| n.path == from))
                    && from.parent() != Some(folder)
                    && !folder.starts_with(from)
            })
    }
    fn drop_entry(
        &mut self,
        drag: &EntryDrag,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.vault.as_ref().is_none_or(|v| v.root != drag.root)
            || !self.can_move_to(&drag.path, &folder)
        {
            return;
        }
        let Some(name) = drag.path.file_name() else {
            return;
        };
        self.dismiss(window, cx);
        self.move_entry(drag.path.clone(), folder.join(name), window, cx);
    }
    fn folder_drop_target(
        &self,
        el: Stateful<Div>,
        folder: PathBuf,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let entity = cx.entity().downgrade();
        let target = folder.clone();
        el.can_drop(move |value, _, cx| {
            value.downcast_ref::<EntryDrag>().is_some_and(|drag| {
                entity.upgrade().is_some_and(|this| {
                    let this = this.read(cx);
                    this.vault.as_ref().is_some_and(|v| v.root == drag.root)
                        && this.can_move_to(&drag.path, &target)
                })
            })
        })
        .drag_over::<EntryDrag>(|style, _, _, _| style.bg(rgb(0x403758)).text_color(rgb(ACCENT)))
        .on_drop(cx.listener(move |this, drag: &EntryDrag, window, cx| {
            this.drop_entry(drag, folder.clone(), window, cx);
            cx.stop_propagation();
        }))
    }
    fn update_move_results(&mut self, cx: &App) {
        let query = self.input.read(cx).buffer.text().trim().to_lowercase();
        if self.move_query == query {
            return;
        }
        self.move_query = query;
        self.move_results = self
            .move_candidates
            .iter()
            .filter(|p| {
                let label = if p.as_os_str().is_empty() {
                    "/".into()
                } else {
                    p.display().to_string()
                };
                label.to_lowercase().contains(&self.move_query)
            })
            .cloned()
            .collect();
        self.move_selected = 0;
        self.move_scroll.scroll_to_item(0, ScrollStrategy::Top);
    }
    fn choose_move_folder(&mut self, folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(from) = self.modal_target.clone() else {
            return;
        };
        if !self.can_move_to(&from, &folder) {
            return;
        }
        let Some(name) = from.file_name() else { return };
        let to = folder.join(name);
        self.move_entry(from, to, window, cx);
    }
    fn render_explorer_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let menu = self.explorer_menu.as_ref().unwrap();
        let mut content = div()
            .id("explorer-menu")
            .debug_selector(|| "explorer-menu".into())
            .w(px(250.))
            .p_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(LINE))
            .bg(rgb(PANEL))
            .shadow_lg()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.explorer_menu = None;
                cx.notify();
            }));
        for (index, (action, label, shortcut)) in self.explorer_actions().into_iter().enumerate() {
            content = content.child(
                div()
                    .id(("explorer-action", index))
                    .debug_selector(move || format!("explorer-action-{index}"))
                    .h(px(32.))
                    .px_3()
                    .rounded_sm()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_sm()
                    .cursor_pointer()
                    .when(index == menu.selected, |el| el.bg(rgb(0x343049)))
                    .hover(|s| s.bg(rgb(0x343049)))
                    .child(label)
                    .child(caption(shortcut))
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if let Some(menu) = &mut this.explorer_menu
                            && menu.selected != index
                        {
                            menu.selected = index;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run_explorer_action(action, window, cx);
                        cx.stop_propagation();
                    })),
            );
        }
        deferred(
            anchored()
                .position(menu.position)
                .snap_to_window_with_margin(px(8.))
                .child(content),
        )
        .with_priority(1)
        .into_any_element()
    }
    fn render_move_picker(&self, cx: &mut Context<Self>) -> AnyElement {
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
                    .w(px(500.))
                    .p_5()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(PANEL))
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_lg().child("Move to folder"))
                    .child(
                        caption(
                            self.modal_target
                                .as_ref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default(),
                        )
                        .truncate(),
                    )
                    .child(
                        div()
                            .h(px(40.))
                            .border_1()
                            .border_color(rgb(ACCENT))
                            .rounded_md()
                            .bg(rgb(BG))
                            .child(self.input.clone()),
                    )
                    .child(caption("Search folders · ↑ ↓ to choose · Enter to move"))
                    .when(self.move_results.is_empty(), |el| {
                        el.child(caption("No matching destination folders"))
                    })
                    .child(
                        uniform_list(
                            "move-folders",
                            self.move_results.len(),
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .map(|index| {
                                        let path = this.move_results[index].clone();
                                        let label = if path.as_os_str().is_empty() {
                                            "/".into()
                                        } else {
                                            path.display().to_string()
                                        };
                                        div()
                                            .id(("move-folder", index))
                                            .debug_selector(move || format!("move-folder-{index}"))
                                            .h(px(34.))
                                            .px_3()
                                            .py_1()
                                            .rounded_md()
                                            .text_sm()
                                            .cursor_pointer()
                                            .when(index == this.move_selected, |el| {
                                                el.bg(rgb(0x343049))
                                            })
                                            .hover(|s| s.bg(rgb(0x303749)))
                                            .child(label)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.choose_move_folder(path.clone(), window, cx)
                                            }))
                                    })
                                    .collect()
                            }),
                        )
                        .h(px(240.))
                        .track_scroll(self.move_scroll.clone()),
                    )
                    .when(self.error, |el| {
                        el.child(
                            div()
                                .text_sm()
                                .text_color(rgb(0xf1b18e))
                                .child(self.status.clone()),
                        )
                    })
                    .child(
                        div().flex().justify_end().child(
                            button("cancel-move", "Cancel").on_click(
                                cx.listener(|this, _, window, cx| this.dismiss(window, cx)),
                            ),
                        ),
                    ),
            )
            .into_any_element()
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
        self.cancel_indexing();
        self.load_cancelled.store(true, Ordering::Relaxed);
        self.managing = true;
        self.load_generation += 1;
        self._load = None;
        self.loading = false;
        self.revision += 1;
        self.note_generation += 1;
        self._note_load = None;
        self.note_loading = false;
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
                        if this.modal == Modal::None {
                            this.tree_focus.focus(window);
                        } else {
                            this.input.read(cx).focus.focus(window);
                        }
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
        self.note_loading = false;
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
        self.explorer_menu = None;
        self.inline_parent = None;
        self.error = false;
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
            let from = self.modal_target.as_ref().unwrap();
            let mut folders = vec![PathBuf::new()];
            folders.extend(self.vault.as_ref().unwrap().folders.iter().cloned());
            folders.retain(|p| from.parent() != Some(p.as_path()) && !p.starts_with(from));
            folders.sort_by_cached_key(|p| p.to_string_lossy().to_lowercase());
            self.move_candidates = folders.clone();
            self.move_results = folders;
            self.move_selected = 0;
            self.move_query.clear();
            self.move_scroll = UniformListScrollHandle::new();
            String::new()
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
        let inline = self.inline_parent.is_some();
        self.modal = Modal::None;
        self.inline_parent = None;
        self.explorer_menu = None;
        if inline {
            self.tree_focus.focus(window);
        } else {
            self.focus_document(window, cx);
        }
        cx.notify();
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.managing {
            return;
        }
        let mut input = self.input.read(cx).buffer.text().trim().to_owned();
        if self.inline_parent.is_some() && matches!(self.modal, Modal::New | Modal::NewFolder) {
            if input.is_empty() {
                self.message("Enter a name", true, cx);
                return;
            }
            input = self
                .inline_parent
                .as_ref()
                .unwrap()
                .join(input)
                .to_string_lossy()
                .into_owned();
        }
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
            Modal::Move => {
                self.update_move_results(cx);
                if let Some(folder) = self.move_results.get(self.move_selected).cloned() {
                    self.choose_move_folder(folder, window, cx);
                }
            }
            Modal::Rename => {
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
                if to.components().count() != 1 {
                    self.message("Enter a single name; use Move to change folders", true, cx);
                    return;
                }
                let to = from.parent().unwrap_or(std::path::Path::new("")).join(to);
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
        if key == "escape" && cx.has_active_drag() {
            cx.stop_active_drag(window);
            self.tab_drop = None;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.explorer_menu.is_some() {
            let action = if command && key == "n" {
                Some(if shift { Modal::NewFolder } else { Modal::New })
            } else if !command && key == "f2" {
                Some(Modal::Rename)
            } else {
                None
            };
            if let Some(action) = action {
                self.run_explorer_action(action, window, cx);
                cx.stop_propagation();
                return;
            }
            let actions = self.explorer_actions();
            let menu = self.explorer_menu.as_mut().unwrap();
            match key {
                "escape" => {
                    self.explorer_menu = None;
                }
                "down" => menu.selected = (menu.selected + 1) % actions.len(),
                "up" => menu.selected = (menu.selected + actions.len() - 1) % actions.len(),
                "enter" | "space" => {
                    let action = actions[menu.selected].0;
                    self.run_explorer_action(action, window, cx);
                }
                _ => (),
            }
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if self.modal != Modal::None {
            if self.modal == Modal::Move && matches!(key, "up" | "down") && !command {
                self.update_move_results(cx);
                let count = self.move_results.len();
                if count > 0 {
                    self.move_selected = if key == "down" {
                        (self.move_selected + 1) % count
                    } else {
                        (self.move_selected + count - 1) % count
                    };
                    self.move_scroll
                        .scroll_to_item(self.move_selected, ScrollStrategy::Center);
                    cx.notify();
                }
                cx.stop_propagation();
                return;
            }
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
        if !command && key == "f2" {
            if !self.tree_focus.is_focused(window) {
                self.navigation.selected = self.tabs.get(self.active).map(|t| t.path.clone());
            }
            self.show_inline(Modal::Rename, window, cx);
            cx.stop_propagation();
            return;
        }
        if !command && shift && key == "f10" && self.tree_focus.is_focused(window) {
            self.open_explorer_menu(
                self.navigation.selected.clone(),
                point(px(24.), px(160.)),
                window,
                cx,
            );
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
                "n" if shift => self.show_inline(Modal::NewFolder, window, cx),
                "n" => self.show_inline(Modal::New, window, cx),
                "tab" if !self.tabs.is_empty() => {
                    let count = self.tabs.len();
                    self.activate_tab(
                        (self.active + if shift { count - 1 } else { 1 }) % count,
                        window,
                        cx,
                    );
                }
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" if !self.tabs.is_empty() => {
                    let index = if key == "9" {
                        self.tabs.len() - 1
                    } else {
                        key.parse::<usize>().unwrap() - 1
                    };
                    self.activate_tab(index, window, cx);
                }
                "o" => self.pick_vault(window, cx),
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
            if self.searching
                || (self.search_results.is_empty()
                    && self.vault.as_ref().is_some_and(|v| !v.content_indexed()))
            {
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
        if prepared.complete && prepared.rows.is_empty() {
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
                let complete = self.tabs[self.active].prepared.complete;
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
                                            if !complete {
                                                this.message(
                                                    "This code block is still loading.",
                                                    false,
                                                    cx,
                                                );
                                                return;
                                            }
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
                let link = s
                    .target
                    .as_deref()
                    .map(|target| self.link_highlight(target));
                (
                    span.range.clone(),
                    HighlightStyle {
                        color: if let Some(link) = &link {
                            link.color
                        } else if s.code {
                            Some(rgb(0xa9d5c3).into())
                        } else {
                            None
                        },
                        underline: link.and_then(|style| style.underline),
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
                    if external_link(target) {
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
        self.performance.update(cx, |bar, _| bar.mark_frame_start());
        self.update_search(window, cx);
        self.update_tree();
        if self.modal == Modal::Move {
            self.update_move_results(cx);
        }
        if !self.managing {
            self.update_document(cx);
        }
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
            .track_scroll(&self.tabs_scroll)
            .overflow_x_scroll()
            .h(px(43.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .bg(rgb(0x181c25))
            .border_b_1()
            .border_color(rgb(LINE));
        for (index, tab) in self.tabs.iter().enumerate() {
            let drop_after = self.tab_drop == Some(index + 1);
            let title = tab
                .path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let drag = TabDrag {
                editor: tab.editor.entity_id(),
                title: title.clone(),
            };
            tabs = tabs.child(
                div()
                    .id(("tab", index))
                    .debug_selector(move || format!("tab-{index}"))
                    .h_full()
                    .min_w(px(120.))
                    .px_3()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_l_2()
                    .border_r_1()
                    .border_color(rgb(LINE))
                    .cursor_pointer()
                    .bg(rgb(if index == self.active { BG } else { 0x181c25 }))
                    .text_sm()
                    .text_color(rgb(if index == self.active { TEXT } else { MUTED }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .max_w(px(180.))
                            .truncate()
                            .child(format!(
                                "{}{}",
                                title,
                                if tab.dirty(cx) { "  •" } else { "" }
                            )),
                    )
                    .tooltip({
                        let path = tab.path.display().to_string();
                        move |_, cx| cx.new(|_| PathTooltip(path.clone())).into()
                    })
                    .on_drag(drag, |drag, _, _, cx| {
                        cx.new(|_| PathTooltip(drag.title.clone()))
                    })
                    .drag_over::<TabDrag>(move |style, _, _, _| {
                        if drop_after {
                            style.border_l_0().border_r_2().border_color(rgb(ACCENT))
                        } else {
                            style.border_l_2().border_r_0().border_color(rgb(ACCENT))
                        }
                    })
                    .on_drag_move(cx.listener(
                        move |this, event: &DragMoveEvent<TabDrag>, _, cx| {
                            if event.bounds.contains(&event.event.position) {
                                let before = index
                                    + usize::from(
                                        event.event.position.x >= event.bounds.center().x,
                                    );
                                if this.tab_drop != Some(before) {
                                    this.tab_drop = Some(before);
                                    cx.notify();
                                }
                            }
                        },
                    ))
                    .on_drop(cx.listener(move |this, drag: &TabDrag, window, cx| {
                        this.reorder_tab(drag.editor, this.tab_drop.unwrap_or(index), cx);
                        this.focus_document(window, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(move |this, _, window, cx| {
                            this.close_tab_at(index, window, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.activate_tab(index, window, cx);
                            window.prevent_default();
                        }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        // Mouse press already activated this tab. Keep click
                        // activation for keyboard/accessibility without logging twice.
                        if this.active != index {
                            this.activate_tab(index, window, cx);
                        }
                    }))
                    .child(
                        div()
                            .id(("close-tab", index))
                            .debug_selector(move || format!("close-tab-{index}"))
                            .size(px(22.))
                            .rounded_sm()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(rgb(MUTED))
                            .hover(|s| s.bg(rgb(0x403748)).text_color(rgb(TEXT)))
                            .child("×")
                            .tooltip(|_, cx| {
                                cx.new(|_| PathTooltip("Close tab · Ctrl W".into())).into()
                            })
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.close_tab_at(index, window, cx);
                                cx.stop_propagation();
                            })),
                    ),
            );
        }
        let end = self.tabs.len();
        tabs = tabs.child(
            div()
                .id("tabs-end")
                .debug_selector(|| "tabs-end".into())
                .min_w(px(36.))
                .flex_1()
                .h_full()
                .border_l_2()
                .border_color(rgb(LINE))
                .drag_over::<TabDrag>(|style, _, _, _| style.border_color(rgb(ACCENT)))
                .on_drop(cx.listener(move |this, drag: &TabDrag, window, cx| {
                    this.reorder_tab(drag.editor, end, cx);
                    this.focus_document(window, cx);
                })),
        );
        main = main.child(tabs);
        if let Some(tab) = self.tabs.get(self.active) {
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
            tools = tools.child(
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
                    )),
            );
            main = main.child(tools);
            let mut panes = div()
                .debug_selector(|| "note-panes".into())
                .flex_1()
                .min_h_0()
                .flex();
            if self.mode != ViewMode::Preview {
                panes = panes.child(
                    div()
                        .debug_selector(|| "source-pane".into())
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(
                            AnyView::from(editor).cached(StyleRefinement::default().size_full()),
                        ),
                );
            }
            if self.mode == ViewMode::Split {
                panes = panes.child(div().w(px(1.)).h_full().bg(rgb(LINE)));
            }
            if self.mode != ViewMode::Source {
                panes = panes.child(
                    div()
                        .debug_selector(|| "preview-pane".into())
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(
                            AnyView::from(self.preview_pane.clone())
                                .cached(StyleRefinement::default().size_full()),
                        ),
                );
            }
            if tab.initial_frame_pending {
                let owner = cx.entity().downgrade();
                let editor_id = tab.editor.entity_id();
                panes = panes.relative().child(
                    canvas(
                        |_, _, _| (),
                        move |_, _, window, cx| {
                            // Schedule from paint, not from the load callback: even a
                            // single worker must draw the opening view before bulk work.
                            window.defer(cx, move |_, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    if let Some(tab) = this
                                        .tabs
                                        .iter_mut()
                                        .find(|tab| tab.editor.entity_id() == editor_id)
                                        && tab.initial_frame_pending
                                    {
                                        tab.initial_frame_pending = false;
                                        this.update_document(cx);
                                        if this.vault.as_ref().is_some_and(|v| !v.content_indexed())
                                        {
                                            cx.notify();
                                        }
                                    }
                                });
                            });
                        },
                    )
                    .absolute()
                    .size_full(),
                );
            }
            main = main.child(panes);
            let rows = self.connections.read(cx).items.len().div_ceil(3).min(3);
            let height = 34. + rows as f32 * 30.;
            main = main.child(
                div().h(px(height)).flex_shrink_0().child(
                    AnyView::from(self.connections.clone())
                        .cached(StyleRefinement::default().size_full()),
                ),
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
                            .on_click(
                                cx.listener(|this, _, window, cx| this.pick_vault(window, cx)),
                            ),
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
                if tab.prepared.complete {
                    format!(
                        "{} words   ·   Ln {line}, Col {column}   ·   UTF-8",
                        tab.prepared.words
                    )
                } else {
                    format!("Ln {line}, Col {column}   ·   UTF-8")
                }
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
                        el.child(
                            div()
                                .w(px(self.navigation.width))
                                .h_full()
                                .flex_shrink_0()
                                .child(
                                    AnyView::from(self.sidebar.clone())
                                        .cached(StyleRefinement::default().size_full()),
                                ),
                        )
                        .child(
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
                    .gap_3()
                    .justify_between()
                    .text_xs()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(rgb(if self.error { 0xf1b18e } else { MUTED }))
                            .child(self.status.clone()),
                    )
                    .child(
                        div().w(px(230.)).h(px(18.)).flex_shrink_0().child(
                            AnyView::from(self.performance.clone())
                                .cached(StyleRefinement::default().size_full()),
                        ),
                    )
                    .child(caption(if self.indexing {
                        format!("Indexing note contents…   ·   {metrics}")
                    } else {
                        metrics
                    })),
            );
        if self.explorer_menu.is_some() && !self.managing {
            root = root.child(self.render_explorer_menu(cx));
        }
        if self.modal == Modal::Move && !self.managing {
            root = root.child(self.render_move_picker(cx));
        }
        if !matches!(self.modal, Modal::None | Modal::Move)
            && self.inline_parent.is_none()
            && !self.managing
        {
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
                    "Open vault by path",
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
                Modal::Move | Modal::None => unreachable!(),
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

struct PendingPerformance {
    action: &'static str,
    target: String,
    started: Instant,
    ready: Instant,
    frame_started: Option<Instant>,
    phases: Vec<(&'static str, Duration)>,
}

struct PerformanceEntry {
    action: &'static str,
    target: String,
    elapsed: Duration,
    phases: Vec<(&'static str, Duration)>,
}

impl PerformanceEntry {
    fn label(&self) -> String {
        format!(
            "{} · {:.1} ms",
            self.action,
            self.elapsed.as_secs_f64() * 1000.
        )
    }
}

#[derive(Default)]
struct PerformanceBar {
    pending: Vec<PendingPerformance>,
    history: std::collections::VecDeque<PerformanceEntry>,
}

impl PerformanceBar {
    fn mark_frame_start(&mut self) {
        let now = Instant::now();
        for sample in &mut self.pending {
            sample.frame_started.get_or_insert(now);
        }
    }
    fn log_text(&self) -> String {
        let mut text = String::from(
            "Action → UI paint (excludes GPU presentation and later background analysis).\nLatest 20 actions, newest first. Click to copy.\n",
        );
        for entry in self.history.iter().rev() {
            text.push_str(&format!("\n{} — {}", entry.label(), entry.target));
            for (phase, elapsed) in &entry.phases {
                text.push_str(&format!(
                    "\n  {phase}: {:.1} ms",
                    elapsed.as_secs_f64() * 1000.
                ));
            }
        }
        text
    }
    fn tooltip_text(&self) -> String {
        let mut text = String::from("Action → UI paint. Click to copy full timings.\n");
        if let Some(latest) = self.history.back() {
            for (phase, elapsed) in &latest.phases {
                text.push_str(&format!(
                    "\n{phase}: {:.1} ms",
                    elapsed.as_secs_f64() * 1000.
                ));
            }
        }
        text.push_str("\n\nRecent actions (newest first):");
        for entry in self.history.iter().rev() {
            let name = std::path::Path::new(&entry.target)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            text.push_str(&format!("\n{} — {name}", entry.label()));
        }
        text
    }
}

impl Render for PerformanceBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.mark_frame_start();
        let pending = std::mem::take(&mut self.pending);
        let owner = cx.entity().downgrade();
        let tooltip = self.tooltip_text();
        div()
            .id("performance-log")
            .debug_selector(|| "performance-log".into())
            .size_full()
            .text_xs()
            .text_color(rgb(ACCENT))
            .cursor_pointer()
            .truncate()
            .child(
                self.history
                    .back()
                    .map(PerformanceEntry::label)
                    .unwrap_or_else(|| "Performance · —".into()),
            )
            .tooltip(move |_, cx| cx.new(|_| PathTooltip(tooltip.clone())).into())
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(cx.listener(|this, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(this.log_text()));
            }))
            .when(!pending.is_empty(), |el| {
                el.relative().child(
                    canvas(
                        |_, _, _| (),
                        move |_, _, window, cx| {
                            // Capture at paint, before deferring bookkeeping. Updating
                            // this isolated view must not invalidate the note panes.
                            let painted = Instant::now();
                            window.defer(cx, move |_, cx| {
                                let _ = owner.update(cx, |bar, cx| {
                                    for mut sample in pending {
                                        let frame_started =
                                            sample.frame_started.unwrap_or(sample.ready);
                                        sample.phases.push((
                                            "Frame wait",
                                            frame_started.duration_since(sample.ready),
                                        ));
                                        sample.phases.push((
                                            "Render",
                                            painted.duration_since(frame_started),
                                        ));
                                        bar.history.push_back(PerformanceEntry {
                                            action: sample.action,
                                            target: sample.target,
                                            elapsed: painted.duration_since(sample.started),
                                            phases: sample.phases,
                                        });
                                        if bar.history.len() > 20 {
                                            bar.history.pop_front();
                                        }
                                    }
                                    cx.notify();
                                });
                            });
                        },
                    )
                    .absolute()
                    .size(px(1.)),
                )
            })
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

// Separate cached views keep scrolling confined to the pane that changed.
struct PreviewPane {
    #[cfg(test)]
    render_count: usize,
    owner: WeakEntity<Workspace>,
}
impl Render for PreviewPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.render_count += 1;
        }
        self.owner
            .update(cx, |workspace, cx| workspace.preview(cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}

#[derive(Clone)]
struct ConnectionSnapshot {
    revision: (u64, u64, bool, bool),
    items: Arc<Vec<Connection>>,
    backlinks: usize,
    tags: usize,
    empty_message: &'static str,
}

enum Connection {
    Note(PathBuf, String),
    Tag(String),
}
struct Connections {
    #[cfg(test)]
    render_count: usize,
    owner: WeakEntity<Workspace>,
    items: Arc<Vec<Connection>>,
    backlinks: usize,
    tags: usize,
    empty_message: &'static str,
    scroll: UniformListScrollHandle,
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
                                                            let query = format!("#{tag}");
                                                            let _ = this.owner.update(
                                                                cx,
                                                                |workspace, cx| {
                                                                    workspace.search.update(
                                                                        cx,
                                                                        |editor, cx| {
                                                                            editor.set_text(
                                                                                &query, cx,
                                                                            )
                                                                        },
                                                                    );
                                                                    workspace
                                                                        .search
                                                                        .read(cx)
                                                                        .focus
                                                                        .focus(window);
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

struct Sidebar {
    #[cfg(test)]
    render_count: usize,
    owner: WeakEntity<Workspace>,
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
