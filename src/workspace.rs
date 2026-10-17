use crate::editor::Editor;
use crate::theme::{MISSING_LINK, MONO_FONT, UI_FONT, missing_link_underline};
use gpui::{prelude::*, *};
use opalglass::{
    buffer::Buffer,
    markdown,
    navigation::{self, SortOrder, State as NavigationState, Tree},
    prepared::{Kind, Prepared, PreviewRow},
    search::{Engine as SearchEngine, Hit as SearchHit, SearchIndex, Snippet, Snippets, Ticket},
    vault::Vault,
};
use std::{
    cell::Cell,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex,
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
const SIDEBAR_RAIL_WIDTH: f32 = 36.;
const SPLIT_DIVIDER_WIDTH: f32 = 7.;

struct Tab {
    path: PathBuf,
    baseline: String,
    external_changed: bool,
    editor: Entity<Editor>,
    baseline_normalized: Arc<String>,
    dirty_cache: Cell<(u64, bool)>,
    prepared: Arc<Prepared>,
    prepared_revision: u64,
    initial_frame_pending: bool,
    pending_heading: Option<String>,
    pending_preview: Option<ListOffset>,
    link_style_revision: (u64, u64),
    requested_revision: u64,
    _prepare: Option<Task<()>>,
    _disk_reload: Option<Task<()>>,
    prepare_cancelled: Arc<AtomicBool>,
    preview_scroll: ListState,
    preview_projection: opalglass::callouts::Projection,
    connections_cache: Option<ConnectionSnapshot>,
    connections_scroll: UniformListScrollHandle,
    _subscription: Subscription,
    _scroll_subscription: Subscription,
    _completion_subscription: Subscription,
    _link_subscription: Subscription,
    _attachment_subscription: Subscription,
}
impl Tab {
    fn saved_preview_offset(&self) -> ListOffset {
        let offset = self.preview_scroll.logical_scroll_top();
        ListOffset {
            item_ix: self.preview_projection.row(offset.item_ix),
            ..offset
        }
    }
    fn restore_preview_offset(&mut self, offset: ListOffset) {
        self.reveal_preview_row(offset.item_ix);
        self.preview_scroll.scroll_to(ListOffset {
            item_ix: self.preview_projection.list_index(offset.item_ix),
            ..offset
        });
    }
    fn install_prepared(&mut self, prepared: Arc<Prepared>, preserve_folds: bool) {
        let offset = self.preview_scroll.logical_scroll_top();
        let byte = self
            .prepared
            .rows
            .get(self.preview_projection.row(offset.item_ix))
            .map_or(0, |r| r.source.start);
        if !preserve_folds {
            self.preview_projection.overrides.clear();
        }
        self.preview_projection
            .rebuild(prepared.rows.len(), &prepared.callouts);
        self.preview_projection.with_footnotes(
            prepared.rows.len(),
            &prepared.footnotes,
            &prepared.callouts,
        );
        self.preview_scroll.splice(
            0..self.preview_scroll.item_count(),
            self.preview_projection.len(prepared.rows.len()),
        );
        let row = prepared.row_at_source(byte);
        self.preview_scroll.scroll_to(ListOffset {
            item_ix: self.preview_projection.list_index(row),
            offset_in_item: offset.offset_in_item,
        });
        self.prepared = prepared;
    }
    fn reveal_preview_row(&mut self, row: usize) {
        let mut changed = false;
        for callout in &self.prepared.callouts {
            if callout.rows.contains(&row)
                && row > callout.rows.start
                && self.preview_projection.collapsed(callout)
            {
                self.preview_projection
                    .overrides
                    .insert(callout.source.start, false);
                changed = true;
            }
        }
        if changed {
            self.preview_projection
                .rebuild(self.prepared.rows.len(), &self.prepared.callouts);
            self.preview_projection.with_footnotes(
                self.prepared.rows.len(),
                &self.prepared.footnotes,
                &self.prepared.callouts,
            );
            self.preview_scroll.splice(
                0..self.preview_scroll.item_count(),
                self.preview_projection.len(self.prepared.rows.len()),
            );
        }
        self.preview_scroll.scroll_to(ListOffset {
            item_ix: self.preview_projection.list_index(row),
            offset_in_item: px(0.),
        });
    }
    fn cancel_opening_analysis(&mut self) {
        if !self.prepared.analysis_complete() {
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
    Delete,
    RestoreTrash,
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

struct SearchRow {
    path: PathBuf,
    title: String,
    hit: Option<Arc<SearchHit>>,
    snippet: Option<Snippet>,
}

#[derive(Default)]
struct LinkCompletion {
    key: Option<(EntityId, u64, usize)>,
    dismissed: Option<(EntityId, u64, usize)>,
    range: std::ops::Range<usize>,
    candidates: Vec<(PathBuf, String)>,
    selected: usize,
    generation: u64,
    task: Option<Task<()>>,
    cancelled: Arc<AtomicBool>,
}
struct NoteFind {
    input: Entity<Editor>,
    open: bool,
    case_sensitive: bool,
    key: Option<(EntityId, u64, String, bool)>,
    matches: Arc<Vec<std::ops::Range<usize>>>,
    selected: Option<usize>,
    generation: u64,
    task: Option<Task<()>>,
    searching: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum PickerMode {
    Commands,
    Bookmarks,
    Templates,
}
#[derive(Clone)]
enum PickerAction {
    Command(opalglass::commands::Command),
    Note(PathBuf),
    Template(PathBuf),
}
#[derive(Clone)]
struct PickerItem {
    title: String,
    detail: String,
    action: PickerAction,
}
pub struct Workspace {
    vault: Option<Vault>,
    tabs: Vec<Tab>,
    history_back: Vec<navigation::SessionTab>,
    history_forward: Vec<navigation::SessionTab>,
    history_restore: Option<navigation::SessionTab>,
    history_request: bool,
    active: usize,
    focus: FocusHandle,
    search: Entity<Editor>,
    find: NoteFind,
    completion: LinkCompletion,
    input: Entity<Editor>,
    modal: Modal,
    inline_parent: Option<PathBuf>,
    explorer_menu: Option<ExplorerMenu>,
    sort_menu: Option<Point<Pixels>>,
    sort_selected: usize,
    auto_revealed: Option<PathBuf>,
    move_candidates: Vec<PathBuf>,
    move_results: Vec<PathBuf>,
    move_query: String,
    move_selected: usize,
    move_scroll: UniformListScrollHandle,
    tab_drop: Option<usize>,
    tabs_scroll: ScrollHandle,
    mode: ViewMode,
    preferences: opalglass::state::Preferences,
    settings_open: bool,
    properties_panel: Option<PropertiesPanel>,
    history_panel: Option<HistoryPanel>,
    _history: Option<Task<()>>,
    comparison: Option<ExternalComparison>,
    _comparison: Option<Task<()>>,
    mentions: Option<MentionsPanel>,
    _mentions: Option<Task<()>>,
    replace_panel: Option<ReplacePanel>,
    _replace: Option<Task<()>>,
    picker: Option<PickerMode>,
    picker_query: Option<String>,
    picker_selected: usize,
    picker_items: Vec<PickerItem>,
    picker_scroll: UniformListScrollHandle,
    outline_open: bool,
    outline_scroll: UniformListScrollHandle,
    sidebar: Entity<Sidebar>,
    preview_pane: Entity<PreviewPane>,
    preview_revision: Option<(EntityId, u64, u64, bool)>,
    connections: Entity<Connections>,
    connections_revision: Option<(EntityId, u64, u64, bool, bool)>,
    #[cfg(debug_assertions)]
    performance: Entity<PerformanceBar>,
    indexing_bar: Entity<IndexingBar>,
    _index_progress: Option<Task<()>>,
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
    search_results: Vec<SearchRow>,
    search_index: Option<Arc<SearchIndex>>,
    search_engine: Option<Arc<SearchEngine>>,
    search_error: Option<String>,
    search_selected: usize,
    snippet_cache: Arc<Mutex<Snippets>>,
    _snippet_task: Option<Task<()>>,
    snippets_loading: bool,
    snippet_requests: Vec<usize>,
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
    split_ratio: f32,
    scroll_locked: bool,
    live_preview: bool,
    split_bounds: Rc<Cell<Bounds<Pixels>>>,
    split_drag_offset: Option<f32>,
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
    _restore: Option<Task<()>>,
    checkpointing: bool,
    checkpoints: std::collections::HashMap<EntityId, (u64, bool, PathBuf)>,
    _checkpoint: Option<Task<()>>,
    restoring: bool,
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

fn external_link(target: &str) -> bool {
    target.starts_with("https://") || target.starts_with("http://") || target.starts_with("mailto:")
}

impl Workspace {
    pub fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state_directory = if cfg!(test) {
            None
        } else {
            navigation::state_directory()
        };
        let preferences = state_directory
            .as_deref()
            .map(opalglass::state::Preferences::load)
            .unwrap_or_default();
        let search = cx.new(|cx| Editor::new("", true, "Search notes…", cx));
        let input = cx.new(|cx| Editor::new("", true, "", cx));
        let find_input = cx.new(|cx| Editor::new("", true, "Find in note…", cx));
        let owner = cx.entity().downgrade();
        let sidebar = cx.new(|_| Sidebar {
            #[cfg(test)]
            render_count: 0,
            owner: owner.clone(),
        });
        let preview_pane = cx.new(|_| PreviewPane {
            pending_sync: None,
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
            cx.observe(&find_input, |_, _, cx| cx.notify()),
            cx.observe(&cx.entity(), |this, _, cx| {
                // Invalidate cached children before GPUI starts the next frame.
                this.update_indexing(cx);
                this.update_link_styles(cx);
                this.sync_auto_reveal();
                this.update_panes(cx);
                this.sidebar.update(cx, |_, cx| cx.notify());
            }),
        ];
        let watch = cx.spawn(async |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(3)).await;
                let Ok(scan) = this.update(cx, |this, cx| {
                    this.persist_navigation(cx);
                    this.checkpoint(cx);
                    (!this.managing && !this.loading && !this.indexing && !this.checkpointing)
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
            history_back: Vec::new(),
            history_forward: Vec::new(),
            history_restore: None,
            history_request: false,
            active: 0,
            focus: cx.focus_handle(),
            search,
            completion: LinkCompletion::default(),
            find: NoteFind {
                input: find_input,
                open: false,
                case_sensitive: false,
                key: None,
                matches: Arc::new(Vec::new()),
                selected: None,
                generation: 0,
                task: None,
                searching: false,
            },
            input,
            modal: Modal::None,
            inline_parent: None,
            explorer_menu: None,
            sort_menu: None,
            sort_selected: 0,
            auto_revealed: None,
            move_candidates: Vec::new(),
            move_results: Vec::new(),
            move_query: String::new(),
            move_selected: 0,
            move_scroll: UniformListScrollHandle::new(),
            tab_drop: None,
            tabs_scroll: ScrollHandle::new(),
            mode: ViewMode::Split,
            preferences: preferences.clone(),
            settings_open: false,
            properties_panel: None,
            history_panel: None,
            _history: None,
            comparison: None,
            _comparison: None,
            mentions: None,
            _mentions: None,
            replace_panel: None,
            _replace: None,
            picker: None,
            picker_query: None,
            picker_selected: 0,
            picker_items: Vec::new(),
            picker_scroll: UniformListScrollHandle::new(),
            outline_open: false,
            outline_scroll: UniformListScrollHandle::new(),
            sidebar,
            preview_pane,
            preview_revision: None,
            connections,
            connections_revision: None,
            #[cfg(debug_assertions)]
            performance: cx.new(|_| PerformanceBar::default()),
            indexing_bar: cx.new(|_| IndexingBar::default()),
            _index_progress: None,
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
            search_index: None,
            search_engine: None,
            search_error: None,
            search_selected: 0,
            snippet_cache: Arc::new(Mutex::new(Snippets::default())),
            _snippet_task: None,
            snippets_loading: false,
            snippet_requests: Vec::new(),
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
            split_ratio: 0.5,
            scroll_locked: false,
            live_preview: preferences.default_live_preview,
            split_bounds: Rc::new(Cell::new(Bounds::default())),
            split_drag_offset: None,
            typeahead: String::new(),
            typeahead_at: std::time::Instant::now(),
            state_directory,
            persisted_navigation: None,
            modal_target: None,
            managing: false,
            _management: None,
            note_generation: 0,
            _note_load: None,
            note_loading: false,
            _restore: None,
            checkpointing: false,
            checkpoints: Default::default(),
            _checkpoint: None,
            restoring: false,
            _subscriptions: subscriptions,
            _watch: watch,
        };
        workspace.focus.focus(window);
        let path = path.or_else(|| {
            workspace
                .state_directory
                .as_ref()
                .and_then(|dir| opalglass::state::last_vault(dir))
        });
        if let Some(path) = path {
            workspace.open_vault(path, window, cx);
        }
        workspace
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
        #[cfg(debug_assertions)]
        self.log_performance_phases(
            action,
            target,
            started,
            vec![("Action", started.elapsed())],
            cx,
        );
        #[cfg(not(debug_assertions))]
        let _ = (action, target, started, cx);
    }
    fn log_performance_phases(
        &self,
        action: &'static str,
        target: String,
        started: Instant,
        phases: Vec<(&'static str, Duration)>,
        cx: &mut Context<Self>,
    ) {
        #[cfg(debug_assertions)]
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
        #[cfg(not(debug_assertions))]
        let _ = (action, target, started, phases, cx);
    }
}

// Feature modules keep workspace state private to this module tree.
#[path = "workspace/attachments.rs"]
mod attachments;
#[path = "workspace/commands.rs"]
mod commands;
#[path = "workspace/comparison.rs"]
mod comparison;
#[path = "workspace/completion.rs"]
mod completion;
#[path = "workspace/controls.rs"]
mod controls;
#[path = "workspace/dialogs.rs"]
mod dialogs;
#[path = "workspace/document.rs"]
mod document;
#[path = "workspace/document_view.rs"]
mod document_view;
#[path = "workspace/explorer.rs"]
mod explorer;
#[path = "workspace/file_actions.rs"]
mod file_actions;
#[path = "workspace/find.rs"]
mod find;
#[path = "workspace/history.rs"]
mod history;
#[path = "workspace/keyboard.rs"]
mod keyboard;
#[path = "workspace/layout.rs"]
mod layout;
#[path = "workspace/loading.rs"]
mod loading;
#[path = "workspace/mentions.rs"]
mod mentions;
#[path = "workspace/navigation.rs"]
mod note_navigation;
#[path = "workspace/panes.rs"]
mod panes;
#[path = "workspace/preferences.rs"]
mod preferences;
#[path = "workspace/properties.rs"]
mod properties;
#[path = "workspace/reading.rs"]
mod reading;
#[path = "workspace/replace.rs"]
mod replace;
#[path = "workspace/search.rs"]
mod search;
#[path = "workspace/session.rs"]
mod session;
#[path = "workspace/status.rs"]
mod status;
#[path = "workspace/tabs.rs"]
mod tabs;
use comparison::*;
use controls::*;
use history::*;
use mentions::*;
use panes::*;
use properties::*;
use replace::*;
use status::*;
