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
    remote_images: Entity<remote_images::Cache>,
    embedded_documents: Entity<embedded_documents::Cache>,
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

impl Workspace {
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
#[path = "workspace/images.rs"]
mod images;
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

#[path = "workspace/tab.rs"]
mod tab;
use tab::Tab;
#[path = "workspace/link_styles.rs"]
mod link_styles;

#[path = "workspace/inline_text.rs"]
mod inline_text;

#[path = "workspace/tables.rs"]
mod tables;

#[path = "workspace/remote_image_view.rs"]
mod remote_image_view;
#[path = "workspace/remote_images.rs"]
mod remote_images;

#[path = "workspace/initialization.rs"]
mod initialization;

#[path = "workspace/compose_embeds.rs"]
mod compose_embeds;
#[path = "workspace/compose_selection.rs"]
mod compose_selection;
#[path = "workspace/embedded_documents.rs"]
mod embedded_documents;
#[path = "workspace/embedded_runtime.rs"]
mod embedded_runtime;
#[path = "workspace/embedded_view.rs"]
mod embedded_view;
#[path = "workspace/reading_actions.rs"]
mod reading_actions;
#[path = "workspace/reading_blocks.rs"]
mod reading_blocks;
#[path = "workspace/reading_composition.rs"]
mod reading_composition;
#[path = "workspace/reading_context.rs"]
mod reading_context;
#[path = "workspace/reading_footnotes.rs"]
mod reading_footnotes;
#[path = "workspace/reading_projection.rs"]
mod reading_projection;
#[path = "workspace/visible_leases.rs"]
mod visible_leases;
