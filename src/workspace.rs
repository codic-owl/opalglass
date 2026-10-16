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
    connections_cache: Option<ConnectionSnapshot>,
    connections_scroll: UniformListScrollHandle,
    _subscription: Subscription,
    _scroll_subscription: Subscription,
    _completion_subscription: Subscription,
    _link_subscription: Subscription,
    _attachment_subscription: Subscription,
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

#[derive(Clone, Copy)]
enum ToolbarIcon {
    Commands,
    Bookmark,
    Calendar,
    Back,
    Forward,
    Settings,
    Outline,
    Locked,
    Unlocked,
    Source,
    Split,
    Reading,
    Save,
    Copy,
    Reload,
    Panel,
    NewNote,
    NewFolder,
    Sort,
    AutoReveal,
    Collapse,
    Expand,
}

fn icon_button(id: &'static str, icon: ToolbarIcon, label: String, enabled: bool) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .size(px(28.))
        .flex_shrink_0()
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .when(enabled, |el| {
            el.cursor_pointer().hover(|s| s.bg(rgb(0x303749)))
        })
        .when(!enabled, |el| el.opacity(0.35))
        .tooltip(move |_, cx| cx.new(|_| PathTooltip(label.clone())).into())
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    // Small native vector icons, using a consistent 24-unit grid.
                    let lines: &[&[(f32, f32)]] = match icon {
                        ToolbarIcon::Commands => &[
                            &[(4., 6.), (10., 12.), (4., 18.)],
                            &[(13., 18.), (21., 18.)],
                        ],
                        ToolbarIcon::Bookmark => &[&[
                            (6., 3.),
                            (18., 3.),
                            (18., 21.),
                            (12., 16.),
                            (6., 21.),
                            (6., 3.),
                        ]],
                        ToolbarIcon::Calendar => &[
                            &[(3., 5.), (21., 5.), (21., 21.), (3., 21.), (3., 5.)],
                            &[(3., 10.), (21., 10.)],
                            &[(8., 2.), (8., 7.)],
                            &[(16., 2.), (16., 7.)],
                        ],
                        ToolbarIcon::Back => &[
                            &[(14., 5.), (7., 12.), (14., 19.)],
                            &[(7., 12.), (21., 12.)],
                        ],
                        ToolbarIcon::Forward => &[
                            &[(10., 5.), (17., 12.), (10., 19.)],
                            &[(3., 12.), (17., 12.)],
                        ],
                        ToolbarIcon::Settings => &[
                            &[(4., 6.), (20., 6.)],
                            &[(4., 12.), (20., 12.)],
                            &[(4., 18.), (20., 18.)],
                            &[(8., 3.), (8., 9.)],
                            &[(16., 9.), (16., 15.)],
                            &[(10., 15.), (10., 21.)],
                        ],
                        ToolbarIcon::Outline => &[
                            &[(3., 5.), (21., 5.)],
                            &[(8., 10.), (21., 10.)],
                            &[(8., 15.), (21., 15.)],
                            &[(3., 20.), (21., 20.)],
                        ],
                        ToolbarIcon::Locked => &[
                            &[(5., 10.), (19., 10.), (19., 21.), (5., 21.), (5., 10.)],
                            &[
                                (8., 10.),
                                (8., 6.),
                                (9., 3.),
                                (15., 3.),
                                (16., 6.),
                                (16., 10.),
                            ],
                            &[(12., 14.), (12., 17.)],
                        ],
                        ToolbarIcon::Unlocked => &[
                            &[(5., 10.), (19., 10.), (19., 21.), (5., 21.), (5., 10.)],
                            &[(8., 10.), (8., 6.), (9., 3.), (15., 3.), (16., 6.)],
                            &[(12., 14.), (12., 17.)],
                        ],
                        ToolbarIcon::Source => &[
                            &[(8., 6.), (2., 12.), (8., 18.)],
                            &[(16., 6.), (22., 12.), (16., 18.)],
                            &[(14., 3.), (10., 21.)],
                        ],
                        ToolbarIcon::Split => &[
                            &[(3., 4.), (21., 4.), (21., 20.), (3., 20.), (3., 4.)],
                            &[(12., 4.), (12., 20.)],
                        ],
                        ToolbarIcon::Reading => &[&[
                            (12., 6.),
                            (9., 4.),
                            (2., 4.),
                            (2., 19.),
                            (9., 19.),
                            (12., 21.),
                            (15., 19.),
                            (22., 19.),
                            (22., 4.),
                            (15., 4.),
                            (12., 6.),
                            (12., 21.),
                        ]],
                        ToolbarIcon::Save => &[
                            &[
                                (3., 3.),
                                (17., 3.),
                                (21., 7.),
                                (21., 21.),
                                (3., 21.),
                                (3., 3.),
                            ],
                            &[(7., 3.), (7., 9.), (16., 9.), (16., 3.)],
                            &[(7., 21.), (7., 14.), (17., 14.), (17., 21.)],
                        ],
                        ToolbarIcon::Copy => &[
                            &[(8., 8.), (21., 8.), (21., 21.), (8., 21.), (8., 8.)],
                            &[(16., 5.), (16., 2.), (2., 2.), (2., 16.), (5., 16.)],
                        ],
                        ToolbarIcon::Reload => &[
                            &[
                                (20., 10.),
                                (18., 6.),
                                (14., 3.),
                                (9., 3.),
                                (5., 5.),
                                (3., 9.),
                                (3., 14.),
                                (6., 19.),
                                (11., 21.),
                                (16., 20.),
                                (20., 16.),
                            ],
                            &[(20., 3.), (20., 10.), (13., 10.)],
                        ],
                        ToolbarIcon::Panel => &[
                            &[(4., 3.), (20., 3.), (20., 21.), (4., 21.), (4., 3.)],
                            &[(9., 3.), (9., 21.)],
                        ],
                        ToolbarIcon::NewNote => &[
                            &[(10., 5.), (4., 5.), (4., 21.), (20., 21.), (20., 14.)],
                            &[
                                (10., 14.),
                                (11., 10.),
                                (19., 2.),
                                (22., 5.),
                                (14., 13.),
                                (10., 14.),
                            ],
                            &[(17., 4.), (20., 7.)],
                        ],
                        ToolbarIcon::NewFolder => &[
                            &[
                                (3., 6.),
                                (9., 6.),
                                (11., 9.),
                                (21., 9.),
                                (21., 20.),
                                (3., 20.),
                                (3., 6.),
                            ],
                            &[(12., 11.), (12., 17.)],
                            &[(9., 14.), (15., 14.)],
                        ],
                        ToolbarIcon::Sort => &[
                            &[(5., 20.), (5., 4.)],
                            &[(2., 7.), (5., 4.), (8., 7.)],
                            &[(11., 6.), (15., 6.)],
                            &[(11., 12.), (18., 12.)],
                            &[(11., 18.), (22., 18.)],
                        ],
                        ToolbarIcon::AutoReveal => &[
                            &[(4., 3.), (20., 3.)],
                            &[(4., 21.), (20., 21.)],
                            &[(4., 7.), (20., 7.), (20., 17.), (4., 17.), (4., 7.)],
                        ],
                        ToolbarIcon::Collapse => &[
                            &[(7., 3.), (12., 8.), (17., 3.)],
                            &[(7., 21.), (12., 16.), (17., 21.)],
                        ],
                        ToolbarIcon::Expand => &[
                            &[(7., 8.), (12., 3.), (17., 8.)],
                            &[(7., 16.), (12., 21.), (17., 16.)],
                        ],
                    };
                    for line in lines {
                        let mut path = PathBuilder::stroke(px(1.5));
                        for (index, &(x, y)) in line.iter().enumerate() {
                            let p = bounds.origin
                                + point(
                                    bounds.size.width * (x / 24.),
                                    bounds.size.height * (y / 24.),
                                );
                            if index == 0 {
                                path.move_to(p);
                            } else {
                                path.line_to(p);
                            }
                        }
                        if let Ok(path) = path.build() {
                            window.paint_path(path, rgb(MUTED));
                        }
                    }
                },
            )
            .size(px(18.)),
        )
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
    fn show_picker(&mut self, mode: PickerMode, window: &mut Window, cx: &mut Context<Self>) {
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
    fn update_picker(&mut self, cx: &App) {
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
    fn accept_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    fn picker_panel(&self, cx: &mut Context<Self>) -> AnyElement {
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
    fn toggle_bookmark(&mut self, cx: &mut Context<Self>) {
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
    fn execute_command(
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
    fn open_daily_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    fn insert_template(&mut self, path: PathBuf, cx: &mut Context<Self>) {
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
    fn close_completion(&mut self, cx: &mut Context<Self>) {
        self.completion.candidates.clear();
        self.completion.key = None;
        self.completion.generation += 1;
        self.completion.task = None;
        self.completion.cancelled.store(true, Ordering::Relaxed);
        for tab in &self.tabs {
            tab.editor.update(cx, |e, _| e.completion_visible = false);
        }
        cx.notify();
    }
    fn update_completion(&mut self, window: &Window, cx: &mut Context<Self>) {
        let context = self
            .tabs
            .get(self.active)
            .filter(|t| t.editor.read(cx).focus.is_focused(window))
            .filter(|_| {
                self.modal == Modal::None
                    && !self.settings_open
                    && !self.managing
                    && self.mode != ViewMode::Preview
            })
            .and_then(|t| {
                t.editor
                    .read(cx)
                    .link_query()
                    .map(|(range, query)| (t.editor.clone(), t.path.clone(), range, query))
            });
        let Some((editor, from, range, query)) = context else {
            if self.completion.key.is_some() {
                self.close_completion(cx);
            }
            return;
        };
        let key = (
            editor.entity_id(),
            editor.read(cx).buffer.revision(),
            editor.read(cx).buffer.cursor(),
        );
        if self.completion.key == Some(key) || self.completion.dismissed == Some(key) {
            return;
        }
        self.close_completion(cx);
        self.completion.key = Some(key);
        self.completion.range = range;
        let generation = self.completion.generation;
        self.completion.cancelled = Arc::new(AtomicBool::new(false));
        let cancelled = self.completion.cancelled.clone();
        let Some(vault) = self.vault.clone() else {
            return;
        };
        let engine = self.search_engine.clone();
        self.completion.task = Some(cx.spawn(async move |this, cx| {
            let candidates = cx
                .background_executor()
                .spawn(async move {
                    let q = query.to_lowercase();
                    let mut candidates = Vec::new();
                    if !q.is_empty()
                        && let Some(engine) = engine
                        && let Ok(results) =
                            engine.search(&query, &Ticket::from_cancelled(cancelled.clone()))
                    {
                        candidates.extend(
                            results
                                .hits
                                .into_iter()
                                .filter(|h| h.tier.contains("title"))
                                .map(|h| (PathBuf::from(h.path), h.title)),
                        );
                    }
                    let mut fallback = vault
                        .notes
                        .iter()
                        .filter_map(|n| {
                            if n.path == from {
                                return None;
                            }
                            let title = n.title.to_lowercase();
                            let path = n.path.to_string_lossy().to_lowercase();
                            (title.contains(&q) || path.contains(&q)).then_some((
                                !title.starts_with(&q),
                                title,
                                n.path.clone(),
                                n.title.clone(),
                            ))
                        })
                        .collect::<Vec<_>>();
                    fallback.sort();
                    for (_, _, path, title) in fallback {
                        if candidates.len() >= 10 {
                            break;
                        }
                        if !candidates.iter().any(|(p, _)| *p == path) {
                            candidates.push((path, title));
                        }
                    }
                    candidates.retain(|(p, _)| *p != from && vault.contains_path(p));
                    candidates.truncate(10);
                    candidates
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.completion.generation != generation || this.completion.key != Some(key) {
                    return;
                }
                editor.update(cx, |e, _| e.completion_visible = !candidates.is_empty());
                this.completion.candidates = candidates;
                this.completion.selected = 0;
                cx.notify();
            });
        }));
    }
    fn accept_completion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, title)) = self
            .completion
            .candidates
            .get(self.completion.selected)
            .cloned()
        else {
            return;
        };
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        if self.completion.key
            != Some((
                tab.editor.entity_id(),
                tab.editor.read(cx).buffer.revision(),
                tab.editor.read(cx).buffer.cursor(),
            ))
        {
            return;
        }
        let Some(vault) = &self.vault else {
            return;
        };
        let target = if vault.resolve(&tab.path, &title).as_ref() == Some(&path) {
            title
        } else {
            format!(
                "/{}",
                path.with_extension("").to_string_lossy().replace('\\', "/")
            )
        };
        let range = self.completion.range.clone();
        tab.editor.update(cx, |e, cx| {
            e.buffer.replace(range, &format!("[[{target}]]"));
            e.jump_to(e.buffer.cursor(), cx);
            e.focus.focus(window);
        });
        self.close_completion(cx);
    }
    fn completion_menu(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let tab = &self.tabs[self.active];
        let point = tab.editor.read(cx).caret_position();
        let size = window.viewport_size();
        let height = self.completion.candidates.len() as f32 * 42. + 8.;
        let top = if f32::from(point.y) + 30. + height < f32::from(size.height) {
            f32::from(point.y) + 30.
        } else {
            (f32::from(point.y) - height).max(0.)
        };
        div()
            .id("link-completion")
            .debug_selector(|| "link-completion".into())
            .absolute()
            .left(px(
                f32::from(point.x).clamp(0., (f32::from(size.width) - 340.).max(0.))
            ))
            .top(px(top))
            .w(px(340.))
            .py_1()
            .rounded_md()
            .bg(rgb(PANEL))
            .border_1()
            .border_color(rgb(LINE))
            .shadow_lg()
            .occlude()
            .children(
                self.completion
                    .candidates
                    .iter()
                    .enumerate()
                    .map(|(i, (path, title))| {
                        div()
                            .id(("link-option", i))
                            .debug_selector(move || format!("link-option-{i}"))
                            .h(px(42.))
                            .px_3()
                            .cursor_pointer()
                            .when(i == self.completion.selected, |el| el.bg(rgb(0x303749)))
                            .hover(|s| s.bg(rgb(0x303749)))
                            .child(div().text_sm().truncate().child(title.clone()))
                            .child(caption(path.display().to_string()).truncate())
                            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                                window.prevent_default();
                                cx.stop_propagation();
                            })
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.completion.selected = i;
                                this.accept_completion(w, cx);
                            }))
                    }),
            )
            .into_any_element()
    }
    fn apply_preferences(&mut self, cx: &mut Context<Self>) {
        for tab in &self.tabs {
            tab.editor.update(cx, |e, cx| {
                e.set_preferences(
                    self.preferences.font_size,
                    self.preferences.line_numbers,
                    cx,
                )
            });
        }
        self.preview_pane.update(cx, |_, cx| cx.notify());
        if let Some(directory) = &self.state_directory
            && let Err(error) = self.preferences.save(directory)
        {
            self.message(format!("Could not save settings: {error}"), true, cx);
        }
        cx.notify();
    }
    fn settings_panel(&self, cx: &mut Context<Self>) -> AnyElement {
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
                    .w(px(460.))
                    .p_6()
                    .rounded_lg()
                    .bg(rgb(PANEL))
                    .border_1()
                    .border_color(rgb(LINE))
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(div().text_xl().child("Settings"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child("Text size")
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        button("font-smaller", "−")
                                            .debug_selector(|| "font-smaller".into())
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.preferences.font_size =
                                                    (this.preferences.font_size - 1.).max(10.);
                                                this.apply_preferences(cx);
                                            })),
                                    )
                                    .child(format!("{} px", self.preferences.font_size as u32))
                                    .child(
                                        button("font-larger", "+")
                                            .debug_selector(|| "font-larger".into())
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.preferences.font_size =
                                                    (this.preferences.font_size + 1.).min(28.);
                                                this.apply_preferences(cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        button(
                            "setting-line-numbers",
                            if self.preferences.line_numbers {
                                "Line numbers: On"
                            } else {
                                "Line numbers: Off"
                            },
                        )
                        .debug_selector(|| "setting-line-numbers".into())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.preferences.line_numbers = !this.preferences.line_numbers;
                            this.apply_preferences(cx);
                        })),
                    )
                    .child(
                        button(
                            "setting-default-editor",
                            if self.preferences.default_live_preview {
                                "Default editing mode: Live Preview"
                            } else {
                                "Default editing mode: Source"
                            },
                        )
                        .debug_selector(|| "setting-default-editor".into())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.preferences.default_live_preview =
                                !this.preferences.default_live_preview;
                            this.apply_preferences(cx);
                        })),
                    )
                    .child(caption(
                        "Default editing mode applies when opening a workspace.",
                    ))
                    .child(
                        button(
                            "setting-autosave",
                            if self.preferences.autosave {
                                "Autosave: On"
                            } else {
                                "Autosave: Off"
                            },
                        )
                        .debug_selector(|| "setting-autosave".into())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.preferences.autosave = !this.preferences.autosave;
                            this.apply_preferences(cx);
                        })),
                    )
                    .child(
                        button("settings-close", "Done")
                            .debug_selector(|| "settings-close".into())
                            .on_click(cx.listener(|this, _, w, cx| {
                                this.settings_open = false;
                                this.focus_document(w, cx);
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }
    fn outline(&self, cx: &mut Context<Self>) -> AnyElement {
        let count = self
            .tabs
            .get(self.active)
            .map_or(0, |t| t.prepared.headings.len());
        div()
            .debug_selector(|| "heading-outline".into())
            .w(px(220.))
            .h_full()
            .flex_shrink_0()
            .border_l_1()
            .border_color(rgb(LINE))
            .flex()
            .flex_col()
            .child(div().p_3().child(caption("OUTLINE")))
            .when(count == 0, |el| {
                el.child(div().p_3().child(caption("No headings in this note")))
            })
            .child(
                uniform_list(
                    "outline-list",
                    count,
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|index| {
                                let tab = &this.tabs[this.active];
                                let (title, row) = &tab.prepared.headings[index];
                                let level =
                                    if let Kind::Heading(level) = tab.prepared.rows[*row].kind {
                                        level
                                    } else {
                                        1
                                    };
                                div()
                                    .id(("outline-heading", index))
                                    .debug_selector(move || format!("outline-heading-{index}"))
                                    .h(px(32.))
                                    .pl(px(12. + (level - 1) as f32 * 12.))
                                    .pr_2()
                                    .text_sm()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(0x303749)))
                                    .child(div().truncate().child(title.clone()))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let Some(tab) = this.tabs.get(this.active) else {
                                            return;
                                        };
                                        let row = tab.prepared.headings[index].1;
                                        let byte = tab.prepared.rows[row].source.start;
                                        tab.editor.update(cx, |e, cx| e.jump_to(byte, cx));
                                        tab.preview_scroll.scroll_to(ListOffset {
                                            item_ix: row,
                                            offset_in_item: px(0.),
                                        });
                                        this.preview_pane.update(cx, |_, cx| cx.notify());
                                        this.focus_document(window, cx);
                                    }))
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(self.outline_scroll.clone())
                .flex_1()
                .min_h_0(),
            )
            .into_any_element()
    }
    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    fn update_find(&mut self, cx: &mut Context<Self>) {
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
    fn reveal_find(&mut self, cx: &mut Context<Self>) {
        let Some(range) = self
            .find
            .selected
            .and_then(|i| self.find.matches.get(i))
            .cloned()
        else {
            return;
        };
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        tab.editor
            .update(cx, |e, cx| e.select_match(range.clone(), cx));
        let row = tab
            .prepared
            .rows
            .partition_point(|r| r.source.start <= range.start)
            .saturating_sub(1);
        tab.preview_scroll.scroll_to(ListOffset {
            item_ix: row,
            offset_in_item: px(0.),
        });
        self.preview_pane.update(cx, |_, cx| cx.notify());
    }
    fn next_find(&mut self, backwards: bool, cx: &mut Context<Self>) {
        let count = self.find.matches.len();
        if count == 0 {
            return;
        }
        let selected = self.find.selected.unwrap_or(0);
        self.find.selected = Some((selected + if backwards { count - 1 } else { 1 }) % count);
        self.reveal_find(cx);
        cx.notify();
    }
    fn find_bar(&self, cx: &mut Context<Self>) -> AnyElement {
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
    fn apply_scan(&mut self, vault: Vault, revision: u64, cx: &mut Context<Self>) {
        if self.managing
            || self.checkpointing
            || self.revision != revision
            || self.vault.as_ref().is_none_or(|v| v.root != vault.root)
        {
            return;
        }
        let mut reload = Vec::new();
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            match vault.notes.iter().find(|n| n.path == tab.path) {
                Some(note) if !note.matches_text(&tab.baseline) => {
                    tab.external_changed = true;
                    if !tab.dirty(cx) {
                        reload.push(index);
                    } else {
                        self.status = "External changes detected. Compare versions to review your edits and the disk version.".into();
                        self.error = true;
                    }
                }
                None => {
                    tab.external_changed = true;
                    self.status =
                        "An open note was removed from disk. Save a copy to preserve its contents."
                            .into();
                    self.error = true;
                }
                _ => tab.external_changed = false,
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
        self.sort_menu = None;
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
                self._restore = None;
                self.restoring = false;
                self.navigation = state;
                self.auto_revealed = None;
                self.persisted_navigation = Some(self.navigation.clone());
                self.tree
                    .rebuild(&vault, &self.navigation.expanded, self.navigation.sort);
                self.tree_revision = self.revision;
                self.vault = Some(vault);
                if let Some(index) = self.search_index.take() {
                    cx.background_executor()
                        .spawn(async move {
                            drop(index);
                        })
                        .detach();
                }
                self.search_engine = None;
                self.search_error = None;
                self.search_results.clear();
                self.search_revision = u64::MAX;
                self.snippet_cache = Arc::new(Mutex::new(Snippets::default()));
                self.tabs.clear();
                self.history_back.clear();
                self.history_forward.clear();
                self.history_restore = None;
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
                self.sort_menu = None;
                self.message("Vault opened", false, cx);
                if let (Some(dir), Some(vault)) = (&self.state_directory, &self.vault)
                    && let Err(error) =
                        opalglass::state::write_json(&dir.join("last-vault.json"), &vault.root)
                {
                    self.message(format!("Could not remember vault: {error}"), true, cx);
                }
                if self.navigation.session_initialized {
                    self.restore_session(window, cx);
                } else if let Some(path) = first {
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
        self._index_progress = None;
        self.indexing = false;
        self.index_revision = u64::MAX;
    }
    fn update_indexing(&mut self, cx: &mut Context<Self>) {
        // Let the first note open before scheduling the bulk index, including
        // on machines with only one background worker.
        if self.loading
            || self.managing
            || self.checkpointing
            || (self.note_loading && self.tabs.is_empty())
            || self
                .tabs
                .get(self.active)
                .is_some_and(|tab| tab.initial_frame_pending)
        {
            return;
        }
        let Some(vault) = &self.vault else { return };
        if self.index_revision == self.revision {
            return;
        }
        let mut vault = vault.clone();
        self.cancel_indexing();
        self.index_cancelled = Arc::new(AtomicBool::new(false));
        let cancelled = self.index_cancelled.clone();
        let revision = self.revision;
        let generation = self.load_generation;
        let existing = self.search_index.clone();
        let cache = if cfg!(test) {
            None
        } else {
            opalglass::search::cache_directory()
        };
        self.index_revision = revision;
        self.indexing = true;
        self.search_error = None;
        let total = vault.notes.len();
        let initial = if vault.content_indexed() {
            VaultIndexProgress::Search(opalglass::search::SyncProgress::Notes {
                completed: 0,
                total,
            })
        } else {
            VaultIndexProgress::Reading {
                completed: 0,
                total,
            }
        };
        let progress = Arc::new(Mutex::new(initial));
        self.indexing_bar.update(cx, |bar, cx| {
            bar.progress = initial;
            cx.notify();
        });
        let observed = progress.clone();
        let bar = self.indexing_bar.downgrade();
        // Update only the small footer entity, at most five times a second.
        // A distinct progress handle per job prevents stale/cancelled updates.
        self._index_progress = Some(cx.spawn(async move |_, cx| {
            loop {
                Timer::after(Duration::from_millis(200)).await;
                let latest = *observed.lock().unwrap();
                if bar
                    .update(cx, |bar, cx| {
                        if bar.progress != latest {
                            bar.progress = latest;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        self._index = Some(cx.spawn(async move |this, cx| {
            let task_cancelled = cancelled.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let changed = if !vault.content_indexed() {
                        vault.refresh_with_progress(&cancelled, |completed| {
                            *progress.lock().unwrap() =
                                VaultIndexProgress::Reading { completed, total };
                        })?
                    } else {
                        false
                    };
                    let index = match existing {
                        Some(index) => index,
                        None => Arc::new(
                            SearchIndex::open(&vault.root, cache.as_deref())
                                .map_err(std::io::Error::other)?,
                        ),
                    };
                    let paths = vault
                        .notes
                        .iter()
                        .map(|n| n.path.clone())
                        .collect::<Vec<_>>();
                    index
                        .synchronize_with_progress(&paths, &cancelled, |state| {
                            *progress.lock().unwrap() = VaultIndexProgress::Search(state);
                        })
                        .map_err(std::io::Error::other)?;
                    let engine = Arc::new(index.snapshot().map_err(std::io::Error::other)?);
                    Ok::<_, std::io::Error>((vault, changed, index, engine))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if task_cancelled.load(Ordering::Relaxed)
                    || this.revision != revision
                    || this.load_generation != generation
                {
                    return;
                }
                this.indexing = false;
                this._index_progress = None;
                match result {
                    Ok((vault, changed, index, engine)) => {
                        if changed {
                            this.apply_scan(vault, revision, cx);
                        }
                        this.search_index = Some(index);
                        this.search_engine = Some(engine);
                        this.index_revision = this.revision;
                        this.search_revision = u64::MAX;
                    }
                    Err(error) => {
                        this.search_error = Some(error.to_string());
                        this.search_revision = u64::MAX;
                        this.message(format!("Could not update search: {error}"), true, cx);
                    }
                }
                cx.notify();
            });
        }));
    }
    fn search_ready(&self) -> bool {
        self.search_engine.is_some()
            && !self.indexing
            && self.index_revision == self.revision
            && self.search_error.is_none()
    }
    fn restore_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(vault) = self.vault.clone() else {
            return;
        };
        let directory = self.state_directory.clone();
        let saved = self.navigation.tabs.clone();
        let active = self.navigation.active_tab.clone();
        let generation = self.note_generation;
        let root = vault.root.clone();
        self.restoring = true;
        self._restore = Some(cx.spawn_in(window, async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    let mut seen = std::collections::HashSet::new();
                    saved
                        .into_iter()
                        .filter_map(|saved| {
                            if !seen.insert(saved.path.clone()) {
                                return None;
                            }
                            let (text, buffer) = opalglass::state::load_document(
                                &vault,
                                &saved.path,
                                directory.as_deref(),
                            )
                            .ok()?;
                            let prepared = Arc::new(Prepared::opening(buffer.text()));
                            Some((saved, text, buffer, prepared))
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.vault.as_ref().is_none_or(|v| v.root != root) {
                    return;
                }
                this.restoring = false;
                // User navigation takes precedence over startup restoration.
                if this.note_generation != generation {
                    return;
                }
                for (saved, text, buffer, prepared) in loaded {
                    this.install_tab(saved.path, text, buffer, prepared, window, cx);
                    let tab = this.tabs.last_mut().unwrap();
                    tab.editor.update(cx, |e, cx| {
                        e.restore_position(saved.anchor, saved.cursor, saved.source, cx)
                    });
                    let offset = ListOffset {
                        item_ix: saved.preview.row,
                        offset_in_item: px(saved.preview.offset),
                    };
                    if tab.prepared.complete {
                        tab.preview_scroll.scroll_to(offset);
                    } else {
                        tab.pending_preview = Some(offset);
                    }
                }
                this.active = this
                    .tabs
                    .iter()
                    .position(|t| Some(&t.path) == active.as_ref())
                    .unwrap_or(0);
                this.focus_document(window, cx);
                cx.notify();
            });
        }));
    }
    fn checkpoint(&mut self, cx: &mut Context<Self>) {
        if self.checkpointing || self.managing || self.loading || self.restoring {
            return;
        }
        let Some(mut vault) = self.vault.clone() else {
            return;
        };
        // Test workspaces opt in by assigning a temporary state directory.
        let Some(directory) = self.state_directory.clone() else {
            return;
        };
        let autosave = self.preferences.autosave;
        let mut requests = Vec::new();
        self.checkpoints
            .retain(|id, _| self.tabs.iter().any(|t| t.editor.entity_id() == *id));
        for tab in &self.tabs {
            let id = tab.editor.entity_id();
            let key = (
                tab.editor.read(cx).buffer.revision(),
                autosave,
                tab.path.clone(),
            );
            if self.checkpoints.get(&id) == Some(&key) {
                continue;
            }
            let dirty = tab.dirty(cx);
            if dirty || self.checkpoints.contains_key(&id) {
                requests.push((
                    id,
                    tab.path.clone(),
                    tab.baseline.clone(),
                    dirty.then(|| tab.disk_text(cx)),
                ));
            }
            self.checkpoints.insert(id, key);
        }
        if requests.is_empty() {
            return;
        }
        self.checkpointing = true;
        let root = vault.root.clone();
        let started = Instant::now();
        self._checkpoint = Some(cx.spawn(async move |this, cx| {
            let (vault, results) = cx
                .background_executor()
                .spawn(async move {
                    let mut results = Vec::new();
                    for (id, path, baseline, text) in requests {
                        let result = (|| -> Result<Option<String>, String> {
                            let Some(text) = text else {
                                opalglass::state::Recovery::remove(&directory, &vault.root, &path)
                                    .map_err(|e| e.to_string())?;
                                return Ok(None);
                            };
                            opalglass::state::Recovery {
                                root: vault.root.clone(),
                                path: path.clone(),
                                baseline: baseline.clone(),
                                text: text.clone(),
                            }
                            .save(&directory)
                            .map_err(|e| format!("Could not save recovery: {e}"))?;
                            if !autosave {
                                return Ok(None);
                            }
                            vault
                                .save(&path, &baseline, &text)
                                .map_err(|e| e.to_string())?;
                            // Retaining a stale record is safe: load_document compares
                            // it with disk before offering recovered edits.
                            let _ =
                                opalglass::state::Recovery::remove(&directory, &vault.root, &path);
                            Ok(Some(text))
                        })();
                        results.push((id, path, baseline, result));
                    }
                    (vault, results)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.checkpointing = false;
                if this.vault.as_ref().is_none_or(|v| v.root != root) {
                    return;
                }
                let mut saved = 0;
                let mut errors = Vec::new();
                for (id, path, baseline, result) in results {
                    match result {
                        Ok(Some(text)) => {
                            if let Some(tab) = this.tabs.iter_mut().find(|t| {
                                t.editor.entity_id() == id
                                    && t.path == path
                                    && t.baseline == baseline
                            }) {
                                // Edits made during the write remain in the buffer
                                // and dirty against the snapshot that reached disk.
                                tab.baseline_normalized =
                                    Arc::new(opalglass::document::normalize(&text));
                                tab.baseline = text;
                                tab.dirty_cache.set((u64::MAX, true));
                            }
                            saved += 1;
                        }
                        Ok(None) => (),
                        Err(error) => {
                            this.checkpoints.remove(&id);
                            errors.push(format!("{}: {error}", path.display()));
                        }
                    }
                }
                if saved > 0 {
                    this.cancel_indexing();
                    this.vault = Some(vault);
                    this.revision += 1;
                }
                if !errors.is_empty() {
                    this.message(
                        format!("Automatic save/recovery: {}", errors.join("; ")),
                        true,
                        cx,
                    );
                } else if saved > 0 {
                    this.message(format!("Automatically saved {saved} note(s)"), false, cx);
                    this.log_performance("Autosave", format!("{saved} note(s)"), started, cx);
                }
                cx.notify();
            });
        }));
    }
    fn persist_navigation(&mut self, cx: &mut Context<Self>) {
        if self.restoring {
            return;
        }
        self.navigation.session_initialized = true;
        self.navigation.tabs = self
            .tabs
            .iter()
            .map(|tab| {
                let editor = tab.editor.read(cx);
                let preview = tab
                    .pending_preview
                    .unwrap_or_else(|| tab.preview_scroll.logical_scroll_top());
                navigation::SessionTab {
                    path: tab.path.clone(),
                    cursor: editor.buffer.cursor(),
                    anchor: editor.buffer.anchor(),
                    source: editor.saved_scroll(),
                    preview: navigation::ScrollPosition {
                        row: preview.item_ix,
                        offset: preview.offset_in_item.into(),
                    },
                }
            })
            .collect();
        self.navigation.active_tab = self.tabs.get(self.active).map(|t| t.path.clone());
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
                self.tree
                    .rebuild(vault, &self.navigation.expanded, self.navigation.sort);
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
    fn sync_auto_reveal(&mut self) {
        if !self.navigation.auto_reveal {
            self.auto_revealed = None;
            return;
        }
        let path = self.tabs.get(self.active).map(|tab| tab.path.clone());
        if path != self.auto_revealed {
            self.auto_revealed = path.clone();
            if let Some(path) = path {
                // Follow navigation without stealing focus, clearing a search,
                // or reopening a sidebar the user deliberately collapsed.
                self.reveal_path(&path);
            }
        }
    }
    fn set_sort(&mut self, order: SortOrder, window: &mut Window, cx: &mut Context<Self>) {
        self.navigation.sort = order;
        self.sort_menu = None;
        self.tree_revision = u64::MAX;
        self.update_tree();
        self.notes_scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(0.)));
        self.focus_document(window, cx);
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
            self.search_selected = 0;
        }
        self.search_cancelled.store(true, Ordering::Relaxed);
        self.search_cancelled = Arc::new(AtomicBool::new(false));
        self._search_task = None;
        self._snippet_task = None;
        self.snippets_loading = false;
        self.snippet_requests.clear();
        self.searching = false;
        if query_changed {
            self.results_scroll = UniformListScrollHandle::new();
        }
        let Some(vault) = self.vault.clone() else {
            return;
        };
        if self.search_query.trim().is_empty() {
            self.search_results.clear();
            return;
        }
        self.searching = true;
        // A saved/externally changed snapshot must finish updating before body
        // results are used. During first indexing, path lookup remains available.
        let tag = query
            .trim()
            .strip_prefix("tag:")
            .or_else(|| query.trim().strip_prefix('#'))
            .filter(|tag| markdown::valid_tag(tag))
            .map(str::to_owned);
        if tag.is_none()
            && self.search_engine.is_some()
            && !self.search_ready()
            && self.search_error.is_none()
        {
            return;
        }
        let engine = self
            .search_ready()
            .then(|| self.search_engine.clone())
            .flatten();
        let query = self.search_query.clone();
        let revision = self.revision;
        let cancelled = self.search_cancelled.clone();
        let task_cancelled = cancelled.clone();
        let started = Instant::now();
        self._search_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            let search_query = query.clone();
            let (results, backend) = cx
                .background_executor()
                .spawn(async move {
                    let started = Instant::now();
                    let result = (|| -> opalglass::search::Result<Vec<SearchRow>> {
                        let ticket = Ticket::from_cancelled(cancelled);
                        ticket.check()?;
                        if let Some(tag) = tag {
                            Ok(vault
                                .notes
                                .iter()
                                .filter(|n| n.has_tag(&tag))
                                .map(|n| SearchRow {
                                    path: n.path.clone(),
                                    title: n.title.clone(),
                                    hit: None,
                                    snippet: None,
                                })
                                .collect())
                        } else if let Some(engine) = engine {
                            let result = engine.search(&search_query, &ticket)?;
                            Ok(result
                                .hits
                                .into_iter()
                                .filter(|hit| vault.contains_path(std::path::Path::new(&hit.path)))
                                .map(|hit| SearchRow {
                                    path: PathBuf::from(&hit.path),
                                    title: hit.title.clone(),
                                    hit: Some(Arc::new(hit)),
                                    snippet: None,
                                })
                                .collect())
                        } else {
                            let needle = search_query.trim().to_lowercase();
                            let mut rows = vault
                                .notes
                                .iter()
                                .filter(|n| {
                                    n.path.to_string_lossy().to_lowercase().contains(&needle)
                                })
                                .map(|n| SearchRow {
                                    path: n.path.clone(),
                                    title: n.title.clone(),
                                    hit: None,
                                    snippet: None,
                                })
                                .collect::<Vec<_>>();
                            rows.sort_by_key(|r| r.title.to_lowercase() != needle);
                            rows.truncate(20);
                            Ok(rows)
                        }
                    })();
                    (result, started.elapsed())
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if task_cancelled.load(Ordering::Relaxed)
                    || this.revision != revision
                    || this.search.read(cx).buffer.text() != query
                {
                    return;
                }
                this.searching = false;
                match results {
                    Ok(results) => {
                        this.search_results = results;
                        this.search_selected = 0;
                    }
                    Err(error) => {
                        this.search_results.clear();
                        this.message(format!("Search: {error}"), true, cx);
                    }
                }
                this.log_performance_phases(
                    "Search",
                    query.clone(),
                    started,
                    vec![("Search backend", backend)],
                    cx,
                );
                if this.open_search_result
                    && (!this.search_results.is_empty() || this.search_ready())
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
    fn queue_search_snippets(&mut self, range: std::ops::Range<usize>, cx: &mut Context<Self>) {
        if self.searching || !self.search_ready() {
            return;
        }
        let Some(engine) = self.search_engine.clone() else {
            return;
        };
        for i in range {
            if self
                .search_results
                .get(i)
                .is_some_and(|r| r.snippet.is_none() && r.hit.is_some())
                && !self.snippet_requests.contains(&i)
            {
                self.snippet_requests.push(i);
            }
        }
        if self.snippets_loading || self.snippet_requests.is_empty() {
            return;
        }
        self.snippets_loading = true;
        let revision = self.revision;
        let query = self.search_query.clone();
        let cancelled = self.search_cancelled.clone();
        let task_cancelled = cancelled.clone();
        let cache = self.snippet_cache.clone();
        self._snippet_task = Some(cx.spawn(async move |this, cx| {
            // UniformList first measures one row, then reports the visible range.
            // Drain after that synchronous render, so both requests form one batch.
            let hits = this
                .update(cx, |this, _| {
                    if task_cancelled.load(Ordering::Relaxed)
                        || this.revision != revision
                        || this.search_query != query
                    {
                        return Vec::new();
                    }
                    std::mem::take(&mut this.snippet_requests)
                        .into_iter()
                        .filter_map(|i| {
                            let row = this.search_results.get(i)?;
                            if row.snippet.is_some() {
                                return None;
                            }
                            Some((i, row.hit.clone()?))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            let started = Instant::now();
            let snippets = cx
                .background_executor()
                .spawn(async move {
                    let ticket = Ticket::from_cancelled(cancelled);
                    let mut cache = cache.lock().map_err(|_| "Snippet cache failed")?;
                    hits.into_iter()
                        .map(|(i, hit)| {
                            ticket.check()?;
                            let snippet = cache
                                .get(&engine, &hit, &ticket)
                                .unwrap_or_else(|_| Snippet::empty("unavailable"));
                            ticket.check()?;
                            Ok((i, snippet))
                        })
                        .collect::<opalglass::search::Result<Vec<_>>>()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if task_cancelled.load(Ordering::Relaxed)
                    || this.revision != revision
                    || this.search_query != query
                {
                    return;
                }
                this.snippets_loading = false;
                match snippets {
                    Ok(snippets) => {
                        for (i, snippet) in snippets {
                            if let Some(row) = this.search_results.get_mut(i) {
                                row.snippet = Some(snippet);
                            }
                        }
                        this.log_performance("Search snippets", query, started, cx);
                    }
                    Err(error) => {
                        // Stop retrying a failed visible batch on every frame.
                        this.search_error = Some(error.to_string());
                        this.message(format!("Could not read search excerpts: {error}"), true, cx);
                    }
                }
                cx.notify();
            });
        }));
    }
    fn open_first_search_result(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self
            .search_results
            .get(self.search_selected)
            .map(|r| r.path.clone())
        {
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
    fn current_location(&self, cx: &App) -> Option<navigation::SessionTab> {
        let tab = self.tabs.get(self.active)?;
        let editor = tab.editor.read(cx);
        let preview = tab
            .pending_preview
            .unwrap_or_else(|| tab.preview_scroll.logical_scroll_top());
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
    fn remember_departure(&mut self, cx: &App) {
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
    fn apply_history_position(&mut self, cx: &mut Context<Self>) {
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
            tab.preview_scroll.scroll_to(pos);
        } else {
            tab.pending_preview = Some(pos);
        }
        self.preview_pane.update(cx, |_, cx| cx.notify());
        cx.notify();
    }
    fn visit_history(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
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
            if heading.is_none() {
                self.activate_tab(index, window, cx);
            } else {
                self.remember_departure(cx);
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
                        this.finish_note_navigation(heading, window, cx);
                    }
                    Err(error) => this.message(format!("Could not open note: {error}"), true, cx),
                }
                phases.push(("UI install", install.elapsed()));
                this.log_performance_phases(action, target, started, phases, cx);
            });
        }));
    }
    fn install_tab(
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
        let complete = prepared.complete;
        self.tabs.push(Tab {
            path,
            baseline: text,
            external_changed: false,
            baseline_normalized: normalized,
            dirty_cache: Cell::new((revision, dirty)),
            preview_scroll: ListState::new(prepared.rows.len(), ListAlignment::Top, px(100.)),
            connections_cache: None,
            connections_scroll: UniformListScrollHandle::new(),
            prepared,
            prepared_revision: revision,
            initial_frame_pending: true,
            pending_heading: None,
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
        self.sort_menu = None;
        self.apply_history_position(cx);
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
        let mut code_blocks: Vec<std::ops::Range<usize>> =
            tab.prepared.frontmatter.iter().cloned().collect();
        for row in &tab.prepared.rows {
            if matches!(row.kind, Kind::Code) {
                if let Some(last) = code_blocks.last_mut()
                    && last.end == row.source.start
                {
                    last.end = row.source.end;
                } else {
                    code_blocks.push(row.source.clone());
                }
            }
        }
        tab.link_style_revision = revision;
        tab.editor.update(cx, |editor, cx| {
            editor.set_code_blocks(code_blocks);
            editor.set_source_links(links, missing);
            cx.notify();
        });
    }
    fn link_highlight(&self, target: &str) -> HighlightStyle {
        let attachment = self
            .vault
            .as_ref()
            .zip(self.tabs.get(self.active))
            .is_some_and(|(vault, tab)| {
                opalglass::attachments::resolve(&vault.root, &tab.path, target).is_ok()
            });
        let missing = !attachment
            && !target.starts_with(markdown::TAG_TARGET_PREFIX)
            && !external_link(target)
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
    fn explorer_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let enabled = self.vault.is_some() && !self.managing;
        let expanded = !self.navigation.expanded.is_empty();
        div()
            .debug_selector(|| "explorer-toolbar".into())
            .h(px(43.))
            .flex_shrink_0()
            .px_2()
            .flex()
            .items_center()
            .justify_end()
            .gap_1()
            .child(
                icon_button(
                    "explorer-new-note",
                    ToolbarIcon::NewNote,
                    "New note".into(),
                    enabled,
                )
                .when(enabled, |el| {
                    el.on_click(cx.listener(|this, _, window, cx| {
                        this.navigation.selected = None;
                        this.show_inline(Modal::New, window, cx);
                    }))
                }),
            )
            .child(
                icon_button(
                    "explorer-new-folder",
                    ToolbarIcon::NewFolder,
                    "New folder".into(),
                    enabled,
                )
                .when(enabled, |el| {
                    el.on_click(cx.listener(|this, _, window, cx| {
                        this.navigation.selected = None;
                        this.show_inline(Modal::NewFolder, window, cx);
                    }))
                }),
            )
            .child(
                icon_button(
                    "explorer-sort",
                    ToolbarIcon::Sort,
                    "Change sort order".into(),
                    enabled,
                )
                .when(enabled, |el| {
                    el.on_click(cx.listener(|this, _, window, cx| {
                        let open = this.sort_menu.is_none();
                        this.dismiss(window, cx);
                        if open {
                            this.sort_menu = Some(window.mouse_position() + point(px(0.), px(12.)));
                            this.sort_selected = SortOrder::ALL
                                .iter()
                                .position(|s| *s == this.navigation.sort)
                                .unwrap_or(0);
                            this.focus.focus(window);
                        }
                        cx.notify();
                    }))
                }),
            )
            .child(
                icon_button(
                    "explorer-auto-reveal",
                    ToolbarIcon::AutoReveal,
                    format!(
                        "Auto-reveal current file · {}",
                        if self.navigation.auto_reveal {
                            "On"
                        } else {
                            "Off"
                        }
                    ),
                    enabled,
                )
                .when(enabled, |el| {
                    el.on_click(cx.listener(|this, _, window, cx| {
                        this.dismiss(window, cx);
                        this.navigation.auto_reveal = !this.navigation.auto_reveal;
                        this.auto_revealed = None;
                        this.sync_auto_reveal();
                        cx.notify();
                    }))
                }),
            )
            .child(
                icon_button(
                    "explorer-collapse-all",
                    if expanded {
                        ToolbarIcon::Collapse
                    } else {
                        ToolbarIcon::Expand
                    },
                    if expanded {
                        "Collapse all"
                    } else {
                        "Expand all"
                    }
                    .into(),
                    enabled,
                )
                .when(enabled, |el| {
                    el.on_click(cx.listener(|this, _, window, cx| {
                        this.dismiss(window, cx);
                        this.search.update(cx, |editor, cx| editor.set_text("", cx));
                        if this.navigation.expanded.is_empty() {
                            this.navigation.expanded = this
                                .vault
                                .as_ref()
                                .unwrap()
                                .folders
                                .iter()
                                .cloned()
                                .collect();
                        } else {
                            this.navigation.expanded.clear();
                            this.navigation.selected = None;
                        }
                        this.tree.flatten(&this.navigation.expanded);
                        this.notes_scroll
                            .0
                            .borrow()
                            .base_handle
                            .set_offset(point(px(0.), px(0.)));
                        cx.notify();
                    }))
                }),
            )
    }
    fn render_sort_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut content = div()
            .id("sort-menu")
            .debug_selector(|| "sort-menu".into())
            .w(px(258.))
            .p_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(LINE))
            .bg(rgb(PANEL))
            .shadow_lg()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.sort_menu = None;
                cx.notify();
            }));
        for (index, order) in SortOrder::ALL.into_iter().enumerate() {
            content = content.child(
                div()
                    .id(("sort-option", index))
                    .debug_selector(move || format!("sort-option-{index}"))
                    .h(px(32.))
                    .px_3()
                    .rounded_sm()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_sm()
                    .cursor_pointer()
                    .when(index == self.sort_selected, |el| el.bg(rgb(0x343049)))
                    .hover(|s| s.bg(rgb(0x343049)))
                    .child(order.label())
                    .child(if self.navigation.sort == order {
                        "✓"
                    } else {
                        ""
                    })
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.sort_selected != index {
                            this.sort_selected = index;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_sort(order, window, cx);
                        cx.stop_propagation();
                    })),
            );
        }
        deferred(
            anchored()
                .position(self.sort_menu.unwrap())
                .snap_to_window_with_margin(px(8.))
                .child(content),
        )
        .with_priority(2)
        .into_any_element()
    }
    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut sidebar = div()
            .w(px(self.navigation.width))
            .h_full()
            .flex_shrink_0()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(LINE))
            .flex()
            .flex_col()
            .child(self.explorer_toolbar(cx))
            .child(
                div()
                    .mx_3()
                    .h(px(40.))
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(BG))
                    .when(self.vault.is_some(), |field| {
                        field.child(self.search.clone())
                    })
                    .when(self.vault.is_none(), |field| {
                        field.opacity(0.5).child(
                            div()
                                .size_full()
                                .px_3()
                                .flex()
                                .items_center()
                                .text_sm()
                                .text_color(rgb(MUTED))
                                .child("Open a vault to search"),
                        )
                    }),
            )
            .child(
                div()
                    .px_4()
                    .pt_3()
                    .pb_2()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(caption(if self.searching {
                        "SEARCHING…".into()
                    } else if !self.search_query.is_empty() && !self.search_ready() {
                        if self.search_error.is_some() {
                            "RESULTS · search unavailable".into()
                        } else {
                            "RESULTS · indexing…".into()
                        }
                    } else if self.search_query.trim().is_empty() {
                        format!(
                            "NOTES · {}",
                            self.vault.as_ref().map_or(0, |v| v.notes.len())
                        )
                    } else {
                        format!(
                            "RESULTS · {}{}",
                            self.search_results.len(),
                            if self.search_results.len() == opalglass::search::query::RESULT_LIMIT {
                                "+"
                            } else {
                                ""
                            }
                        )
                    })),
            );
        let searching = !self.search_query.trim().is_empty();
        let inline_index = (!searching).then(|| self.inline_insert_index()).flatten();
        let notes = uniform_list(
            if searching { "search-results" } else { "notes" },
            if searching {
                self.search_results.len()
            } else {
                self.tree.visible.len() + usize::from(inline_index.is_some())
            },
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                if searching {
                    this.queue_search_snippets(range.clone(), cx);
                }
                range
                    .filter_map(|index| {
                        if inline_index == Some(index) {
                            return Some(this.inline_row());
                        }
                        let row_index =
                            index - usize::from(inline_index.is_some_and(|i| index > i));
                        let (path, title, folder, depth) = if searching {
                            let row = this.search_results.get(index)?;
                            (row.path.clone(), row.title.clone(), false, 0)
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
                                .h(px(if searching { 98. } else { 32. }))
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
                                    let row = &this.search_results[index];
                                    let excerpt = row.snippet.as_ref().map(|snippet| {
                                        let highlights =
                                            snippet.highlights.iter().cloned().map(|range| {
                                                (
                                                    range,
                                                    HighlightStyle {
                                                        color: Some(rgb(ACCENT).into()),
                                                        font_weight: Some(FontWeight::BOLD),
                                                        ..Default::default()
                                                    },
                                                )
                                            });
                                        let fragment = match snippet.status {
                                            "stale" | "missing" => {
                                                "Note changed; waiting for refresh…".to_owned()
                                            }
                                            "unavailable" | "window_miss" | "positions_limit" => {
                                                "Excerpt unavailable".to_owned()
                                            }
                                            _ => snippet.fragment.replace(['\n', '\r', '\t'], " "),
                                        };
                                        div()
                                            .text_xs()
                                            .text_color(rgb(MUTED))
                                            .h(px(36.))
                                            .overflow_hidden()
                                            .child(
                                                StyledText::new(SharedString::from(fragment))
                                                    .with_highlights(highlights),
                                            )
                                    });
                                    el.child(caption(path.display().to_string()).truncate())
                                        .children(excerpt)
                                        .when(index == this.search_selected, |el| {
                                            el.bg(rgb(0x303047))
                                        })
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
                                    if this.managing || searching && this.searching {
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
    fn sync_reading_scroll(&mut self, cx: &mut Context<Self>) {
        if !self.scroll_locked || self.mode != ViewMode::Split {
            return;
        }
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let Some((revision, byte)) = tab.editor.read(cx).scroll_anchor else {
            return;
        };
        if revision != tab.prepared_revision || tab.prepared.rows.is_empty() {
            return;
        }
        let index = tab
            .prepared
            .rows
            .partition_point(|row| row.source.start as f64 <= byte)
            .saturating_sub(1);
        let range = &tab.prepared.rows[index].source;
        let fraction =
            ((byte - range.start as f64) / range.len().max(1) as f64).clamp(0., 1.) as f32;
        let height = tab
            .preview_scroll
            .bounds_for_item(index)
            .map_or(px(0.), |bounds| bounds.size.height);
        tab.preview_scroll.scroll_to(ListOffset {
            item_ix: index,
            offset_in_item: height * fraction,
        });
        let scroll = tab.preview_scroll.clone();
        self.preview_pane.update(cx, |pane, cx| {
            // An offscreen row has no measured height yet. Correct its fractional
            // offset after it is laid out, without measuring the rest of the note.
            pane.pending_sync = Some((scroll, index, fraction));
            cx.notify();
        });
    }
    fn update_panes(&mut self, cx: &mut Context<Self>) {
        self.sync_reading_scroll(cx);
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
                tab.preview_scroll
                    .scroll_to(tab.pending_preview.take().unwrap_or(offset));
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
        if self.checkpointing {
            self.message("Finishing automatic save; try again in a moment", false, cx);
            return;
        }
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
                        tab.external_changed = false;
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
                        if let (Some(dir), Some(vault)) = (&this.state_directory, &this.vault) {
                            let _ = opalglass::state::Recovery::remove(dir, &vault.root, &tab.path);
                        }
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
    fn toggle_live_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let started = Instant::now();
        self.live_preview = !self.live_preview;
        for tab in &self.tabs {
            tab.editor.update(cx, |editor, cx| {
                editor.set_live_preview(self.live_preview, cx)
            });
        }
        self.focus_document(window, cx);
        cx.notify();
        self.log_performance(
            "Change editor",
            if self.live_preview {
                "Live Preview"
            } else {
                "Raw Source"
            }
            .into(),
            started,
            cx,
        );
    }
    fn set_mode(&mut self, mode: ViewMode, window: &mut Window, cx: &mut Context<Self>) {
        let started = Instant::now();
        self.mode = mode;
        self.split_drag_offset = None;
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
        if self.checkpointing {
            self.message("Finishing automatic save; try again in a moment", false, cx);
            return false;
        }
        let started = Instant::now();
        let Some(tab) = self.tabs.get(index) else {
            return true;
        };
        if !tab.dirty(cx) {
            if let (Some(dir), Some(vault)) = (&self.state_directory, &self.vault) {
                let _ = opalglass::state::Recovery::remove(dir, &vault.root, &tab.path);
            }
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
                self.tabs[index].external_changed = false;
                self.tabs[index].baseline = text;
                self.tabs[index].baseline_normalized =
                    self.tabs[index].editor.read(cx).buffer.snapshot();
                self.tabs[index]
                    .dirty_cache
                    .set((self.tabs[index].editor.read(cx).buffer.revision(), false));
                if let Some(dir) = &self.state_directory {
                    let _ = opalglass::state::Recovery::remove(
                        dir,
                        &vault.root,
                        &self.tabs[index].path,
                    );
                }
                self.message("Saved to disk", false, cx);
                self.log_performance("Save note", target, started, cx);
                true
            }
            Err(error) => {
                if matches!(error, opalglass::vault::SaveError::Conflict) {
                    self.tabs[index].external_changed = true;
                }
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
        if self.checkpointing {
            self.message("Finishing automatic save; try again in a moment", false, cx);
            return false;
        }
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
        self.sort_menu = None;
        if !self.save_tab(index, cx) {
            self.activate_tab(index, window, cx);
            self.log_performance("Close blocked", target, started, cx);
            return;
        }
        self.note_generation += 1;
        self._note_load = None;
        self.note_loading = false;
        if self.active == index {
            self.remember_departure(cx);
        }
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
        if self.active != index {
            self.remember_departure(cx);
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
        self.sort_menu = None;
        self.apply_history_position(cx);
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
        if menu.target.is_some() {
            actions.push((Modal::Delete, "Move to trash…", "Delete"));
        }
        actions.push((Modal::RestoreTrash, "Restore last deleted item", ""));
        actions
    }
    fn run_explorer_action(&mut self, action: Modal, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.explorer_menu.take() else {
            return;
        };
        self.navigation.selected = menu.target;
        if action == Modal::RestoreTrash {
            self.change_trash(None, window, cx);
        } else if matches!(action, Modal::Move | Modal::Delete) {
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
    fn change_trash(
        &mut self,
        deleted: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.managing || self.checkpointing {
            return;
        }
        if let Some(path) = &deleted {
            for index in 0..self.tabs.len() {
                if self.tabs[index].path.starts_with(path) && !self.save_tab(index, cx) {
                    self.activate_tab(index, window, cx);
                    return;
                }
            }
        }
        let Some(mut vault) = self.vault.clone() else {
            return;
        };
        self.cancel_indexing();
        self.note_generation += 1;
        self._note_load = None;
        self.note_loading = false;
        self.managing = true;
        self.revision += 1;
        self.focus.focus(window);
        self.message(
            if deleted.is_some() {
                "Moving to trash…"
            } else {
                "Restoring from trash…"
            },
            false,
            cx,
        );
        self._management = Some(cx.spawn_in(window, async move |this, cx| {
            let path = deleted.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let path = if let Some(path) = path {
                        vault.trash_entry(&path)?;
                        path
                    } else {
                        vault.restore_last_trashed()?
                    };
                    Ok::<_, std::io::Error>((vault, path))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.managing = false;
                match result {
                    Ok((vault, path)) => {
                        let active = this.tabs.get(this.active).map(|t| t.editor.entity_id());
                        if deleted.is_some() {
                            this.tabs.retain(|t| !t.path.starts_with(&path));
                            this.navigation.expanded.retain(|p| !p.starts_with(&path));
                            this.navigation.selected = None;
                        }
                        this.active = this
                            .tabs
                            .iter()
                            .position(|t| Some(t.editor.entity_id()) == active)
                            .unwrap_or(this.active.min(this.tabs.len().saturating_sub(1)));
                        this.vault = Some(vault);
                        this.revision += 1;
                        this.dismiss(window, cx);
                        if deleted.is_none() {
                            this.reveal_path(&path);
                        }
                        this.message(
                            format!(
                                "{} {}",
                                if deleted.is_some() {
                                    "Moved to trash:"
                                } else {
                                    "Restored:"
                                },
                                path.display()
                            ),
                            false,
                            cx,
                        );
                        this.persist_navigation(cx);
                    }
                    Err(error) => {
                        this.message(format!("Could not change trash: {error}"), true, cx)
                    }
                }
            });
        }));
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
                                .filter(|n| !n.matches_text(&baseline))
                                .map(|_| {
                                    vault.read(&path).map(|text| {
                                        let buffer =
                                            Buffer::new(opalglass::document::normalize(&text));
                                        let prepared = Arc::new(Prepared::new(buffer.text()));
                                        (text, buffer, prepared)
                                    })
                                })
                                .transpose();
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
                        let mut reload_error = None;
                        for (tab, (path, replacement)) in this.tabs.iter_mut().zip(ready) {
                            tab.path = path;
                            let replacement = match replacement {
                                Ok(replacement) => replacement,
                                Err(error) => {
                                    // The move already committed. Keep the tab
                                    // at its new path with its buffer intact.
                                    reload_error = Some(format!(
                                        "Moved, but could not reload {}: {error}",
                                        tab.path.display()
                                    ));
                                    None
                                }
                            };
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
                        for location in this
                            .history_back
                            .iter_mut()
                            .chain(this.history_forward.iter_mut())
                        {
                            location.path =
                                opalglass::vault::moved_path(&location.path, &from, &to);
                        }
                        for path in &mut this.navigation.bookmarks {
                            *path = opalglass::vault::moved_path(path, &from, &to);
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
                        if let Some(error) = reload_error {
                            this.message(error, true, cx);
                        } else {
                            this.message("Moved; wiki links and open tabs updated", false, cx);
                        }
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
            Modal::New
                | Modal::Copy
                | Modal::NewFolder
                | Modal::Rename
                | Modal::Move
                | Modal::Delete
        ) && self.vault.is_none()
        {
            self.message("Open a vault first", true, cx);
            return;
        }
        if matches!(modal, Modal::Copy | Modal::Reload) && self.tabs.is_empty() {
            return;
        }
        self.explorer_menu = None;
        self.sort_menu = None;
        self.inline_parent = None;
        self.error = false;
        self.modal_target = self.navigation.selected.clone();
        if matches!(modal, Modal::Rename | Modal::Move | Modal::Delete)
            && self.modal_target.is_none()
        {
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
        if modal == Modal::Delete {
            self.focus.focus(window);
        } else {
            self.input.read(cx).focus.focus(window);
        }
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
        self.sort_menu = None;
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
            Modal::Delete => {
                if let Some(path) = self.modal_target.clone() {
                    self.change_trash(Some(path), window, cx);
                }
            }
            Modal::RestoreTrash => self.change_trash(None, window, cx),
            Modal::None => (),
        }
    }
    fn filter_tag(&mut self, tag: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.navigation.hidden = false;
        self.search
            .update(cx, |editor, cx| editor.set_text(&format!("tag:{tag}"), cx));
        self.search.read(cx).focus.focus(window);
        cx.notify();
    }
    fn follow(&mut self, target: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tag) = target.strip_prefix(markdown::TAG_TARGET_PREFIX) {
            self.filter_tag(tag, window, cx);
            return;
        }
        if let (Some(vault), Some(tab)) = (&self.vault, self.tabs.get(self.active)) {
            if let Ok(path) = opalglass::attachments::resolve(&vault.root, &tab.path, &target) {
                cx.open_with_system(&path);
                return;
            }
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
        if self.replace_panel.is_some() {
            if key == "escape" {
                self.replace_panel = None;
                self._replace = None;
                self.focus_document(window, cx);
                cx.notify();
                cx.stop_propagation();
            }
            return;
        }
        if command && shift && key == "h" {
            self.open_replace(window, cx);
            cx.stop_propagation();
            return;
        }
        if self.mentions.is_some() {
            if key == "escape" {
                self.mentions = None;
                self._mentions = None;
                self.focus_document(window, cx);
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        if self.comparison.is_some() {
            if key == "escape" {
                self.comparison = None;
                self.focus_document(window, cx);
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        if self.properties_panel.is_some() {
            if key == "escape" {
                self.properties_panel = None;
                self.focus_document(window, cx);
                cx.notify();
                cx.stop_propagation();
            } else if command && key == "enter" {
                self.apply_properties(window, cx);
                cx.stop_propagation();
            }
            return;
        }
        if self.picker.is_some() {
            self.update_picker(cx);
            let count = self.picker_items.len();
            match key {
                "escape" => {
                    self.picker = None;
                    self.focus_document(window, cx);
                }
                "down" if count > 0 => self.picker_selected = (self.picker_selected + 1) % count,
                "up" if count > 0 => {
                    self.picker_selected = (self.picker_selected + count - 1) % count
                }
                "enter" => self.accept_picker(window, cx),
                _ => return,
            }
            self.picker_scroll
                .scroll_to_item(self.picker_selected, ScrollStrategy::Center);
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if self.settings_open {
            if key == "escape" {
                self.settings_open = false;
                self.focus_document(window, cx);
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        if key == "escape" && cx.has_active_drag() {
            cx.stop_active_drag(window);
            self.tab_drop = None;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.sort_menu.is_some() {
            match key {
                "escape" => {
                    self.sort_menu = None;
                    self.focus_document(window, cx);
                }
                "down" => self.sort_selected = (self.sort_selected + 1) % SortOrder::ALL.len(),
                "up" => {
                    self.sort_selected =
                        (self.sort_selected + SortOrder::ALL.len() - 1) % SortOrder::ALL.len()
                }
                "enter" | "space" => self.set_sort(SortOrder::ALL[self.sort_selected], window, cx),
                _ => (),
            }
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
                    self.sort_menu = None;
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
        if event.keystroke.modifiers.alt
            && (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
        {
            match key {
                "d" => self.open_daily_note(window, cx),
                "t" => self.show_picker(PickerMode::Templates, window, cx),
                _ => return,
            }
            cx.stop_propagation();
            return;
        }
        if event.keystroke.modifiers.alt && matches!(key, "left" | "right") {
            self.visit_history(key == "right", window, cx);
            cx.stop_propagation();
            return;
        }
        if self.find.open && !command {
            if key == "escape" {
                self.close_find(window, cx);
                cx.stop_propagation();
                return;
            }
            if key == "f3" || (key == "enter" && self.find.input.read(cx).focus.is_focused(window))
            {
                self.next_find(shift, cx);
                cx.stop_propagation();
                return;
            }
        }
        if !command && key == "delete" && self.tree_focus.is_focused(window) {
            self.show_modal(Modal::Delete, window, cx);
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
                "," => {
                    self.settings_open = true;
                    self.focus.focus(window);
                    cx.notify();
                }
                "o" if shift => {
                    self.outline_open = !self.outline_open;
                    cx.notify();
                }
                "o" => self.pick_vault(window, cx),
                "f" if !shift => self.open_find(window, cx),
                "p" if shift => self.show_picker(PickerMode::Commands, window, cx),
                "d" if shift => self.toggle_bookmark(cx),
                "p" | "f" if self.vault.is_some() => {
                    self.navigation.hidden = false;
                    self.search.update(cx, |editor, cx| {
                        editor.buffer.select(0, editor.buffer.text().len());
                        cx.notify();
                    });
                    self.search.read(cx).focus.focus(window);
                }
                "w" => self.close_tab(window, cx),
                "e" if shift => self.toggle_live_preview(window, cx),
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
        } else if matches!(key, "up" | "down")
            && self.search.read(cx).focus.is_focused(window)
            && !self.search_results.is_empty()
        {
            if key == "down" {
                self.search_selected =
                    (self.search_selected + 1).min(self.search_results.len() - 1);
            } else {
                self.search_selected = self.search_selected.saturating_sub(1);
            }
            self.results_scroll
                .scroll_to_item(self.search_selected, ScrollStrategy::Center);
            cx.stop_propagation();
            cx.notify();
        } else if key == "enter" && self.search.read(cx).focus.is_focused(window) {
            self.update_search(window, cx);
            if self.searching || (self.search_results.is_empty() && !self.search_ready()) {
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
        .text_size(px(16. * self.preferences.font_size / 14.))
        .line_height(relative(1.7))
        .into_any_element()
    }
    fn preview_row(&self, index: usize, row: &PreviewRow, cx: &mut Context<Self>) -> AnyElement {
        if row.spans.len() == 1
            && row.spans[0].style.image
            && let Some(target) = row.spans[0].style.target.as_deref()
            && let (Some(vault), Some(tab)) = (&self.vault, self.tabs.get(self.active))
            && let Ok(path) = opalglass::attachments::resolve(&vault.root, &tab.path, target)
        {
            return div()
                .id(("attachment-image", index))
                .my_3()
                .flex()
                .flex_col()
                .gap_2()
                .debug_selector(move || format!("attachment-image-{index}"))
                .child(
                    img(path.clone())
                        .w_full()
                        .h(px(320.))
                        .object_fit(ObjectFit::Contain),
                )
                .child(caption(row.text.to_string()))
                .cursor_pointer()
                .on_click(move |_, _, cx| cx.open_with_system(&path))
                .into_any_element();
        }

        let mut style = match row.kind {
            Kind::Heading(level) => div()
                .line_height(relative(1.3))
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(match level {
                    1 => 32.,
                    2 => 25.,
                    3 => 21.,
                    _ => 18.,
                } * self.preferences.font_size
                    / 14.))
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
                            .text_size(px(self.preferences.font_size))
                            .line_height(relative(1.6))
                            .text_color(rgb(0xc2d8ce))
                            .child(
                                StyledText::new(SharedString::from(row.text.clone()))
                                    .with_highlights(if self.find.open {
                                        opalglass::editing::find(
                                            &row.text,
                                            self.find.input.read(cx).buffer.text(),
                                            self.find.case_sensitive,
                                        )
                                        .into_iter()
                                        .map(|r| {
                                            (
                                                r,
                                                HighlightStyle {
                                                    background_color: Some(rgb(0x655224).into()),
                                                    ..Default::default()
                                                },
                                            )
                                        })
                                        .collect::<Vec<_>>()
                                    } else {
                                        Vec::new()
                                    }),
                            ),
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
        let highlights = if self.find.open {
            let matches = opalglass::editing::find(
                &row.text,
                self.find.input.read(cx).buffer.text(),
                self.find.case_sensitive,
            );
            let mut cuts = vec![0, row.text.len()];
            for (r, _) in &highlights {
                cuts.extend([r.start, r.end]);
            }
            for r in &matches {
                cuts.extend([r.start, r.end]);
            }
            cuts.sort_unstable();
            cuts.dedup();
            cuts.windows(2)
                .filter(|p| p[0] < p[1])
                .map(|p| {
                    let mut style = highlights
                        .iter()
                        .find(|(r, _)| r.contains(&p[0]))
                        .map_or(HighlightStyle::default(), |(_, s)| *s);
                    if matches.iter().any(|r| r.contains(&p[0])) {
                        style.background_color = Some(rgb(0x655224).into());
                    }
                    (p[0]..p[1], style)
                })
                .collect()
        } else {
            highlights
        };
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
        #[cfg(debug_assertions)]
        self.performance.update(cx, |bar, _| bar.mark_frame_start());
        self.update_picker(cx);
        self.update_search(window, cx);
        self.update_find(cx);
        self.update_completion(window, cx);
        self.update_tree();
        if self.modal == Modal::Move {
            self.update_move_results(cx);
        }
        if !self.managing {
            self.update_document(cx);
        }
        let mut main = div().flex_1().min_w_0().h_full().flex().flex_col();
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
                        if this.managing || index >= this.tabs.len() {
                            return;
                        }
                        if this.active != index {
                            this.activate_tab(index, window, cx);
                        }
                        this.reveal_active(window, cx);
                        this.focus_document(window, cx);
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
                .gap_3()
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .child(
                            icon_button(
                                "history-back",
                                ToolbarIcon::Back,
                                "Back · Alt ←".into(),
                                !self.history_back.is_empty(),
                            )
                            .on_click(
                                cx.listener(|this, _, w, cx| this.visit_history(false, w, cx)),
                            ),
                        )
                        .child(
                            icon_button(
                                "history-forward",
                                ToolbarIcon::Forward,
                                "Forward · Alt →".into(),
                                !self.history_forward.is_empty(),
                            )
                            .on_click(
                                cx.listener(|this, _, w, cx| this.visit_history(true, w, cx)),
                            ),
                        ),
                )
                .child(
                    caption(path.display().to_string())
                        .flex_1()
                        .min_w_0()
                        .truncate(),
                );
            let mut modes = div().flex().flex_shrink_0().gap_1().child(
                icon_button(
                    "properties",
                    ToolbarIcon::Sort,
                    "Edit properties".into(),
                    true,
                )
                .on_click(cx.listener(|this, _, window, cx| this.open_properties(window, cx))),
            );
            for (id, label, icon, mode) in [
                ("source", "Source", ToolbarIcon::Source, ViewMode::Source),
                ("split", "Split", ToolbarIcon::Split, ViewMode::Split),
                (
                    "reading",
                    "Reading",
                    ToolbarIcon::Reading,
                    ViewMode::Preview,
                ),
            ] {
                let tooltip = if self.mode == mode {
                    format!("{label} · Current view · Ctrl E to cycle views")
                } else {
                    format!("Switch to {label} · Ctrl E to cycle views")
                };
                modes = modes.child(icon_button(id, icon, tooltip, true).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.set_mode(mode, window, cx);
                    },
                )));
            }
            if self.mode != ViewMode::Preview {
                modes = modes.child(
                    icon_button(
                        "toggle-live-preview",
                        ToolbarIcon::NewNote,
                        if self.live_preview {
                            "Live Preview · Switch to raw Source · Ctrl Shift E"
                        } else {
                            "Raw Source · Enable Live Preview · Ctrl Shift E"
                        }
                        .into(),
                        true,
                    )
                    .on_click(
                        cx.listener(|this, _, window, cx| this.toggle_live_preview(window, cx)),
                    ),
                );
            }
            modes = modes.child(
                icon_button(
                    "toggle-outline",
                    ToolbarIcon::Outline,
                    "Heading outline · Ctrl Shift O".into(),
                    true,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.outline_open = !this.outline_open;
                    cx.notify();
                })),
            );
            tools = tools.child(
                modes
                    .child(div().w(px(8.)))
                    .child(
                        icon_button("save", ToolbarIcon::Save, "Save · Ctrl S".into(), true)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.save_tab(this.active, cx);
                            })),
                    )
                    .child(
                        icon_button(
                            "copy",
                            ToolbarIcon::Copy,
                            "Save copy · Ctrl Shift S".into(),
                            true,
                        )
                        .on_click(
                            cx.listener(|this, _, w, cx| this.show_modal(Modal::Copy, w, cx)),
                        ),
                    )
                    .child(
                        icon_button(
                            "reload",
                            ToolbarIcon::Reload,
                            "Reload from disk".into(),
                            true,
                        )
                        .on_click(
                            cx.listener(|this, _, w, cx| this.show_modal(Modal::Reload, w, cx)),
                        ),
                    ),
            );
            main = main.child(tools);
            if tab.external_changed {
                main = main.child(
                    div()
                        .px_4()
                        .py_2()
                        .bg(rgb(0x302b25))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(caption(
                            "This note changed outside Opalglass. Your edits are preserved.",
                        ))
                        .child(
                            button("compare-external", "Compare versions")
                                .debug_selector(|| "compare-external".into())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_comparison(window, cx)
                                })),
                        ),
                );
            }
            if self.find.open {
                main = main.child(self.find_bar(cx));
            }
            let mut panes = div()
                .id("attachment-drop-zone")
                .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(rgb(0x28283b)))
                .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                    if let Some(tab) = this.tabs.get(this.active) {
                        tab.editor.update(cx, |editor, cx| {
                            editor.place_drop_caret(window.mouse_position(), cx)
                        });
                        this.import_attachments(
                            tab.editor.clone(),
                            paths
                                .paths()
                                .iter()
                                .cloned()
                                .map(opalglass::attachments::Input::File)
                                .collect(),
                            window,
                            cx,
                        );
                    }
                }))
                .debug_selector(|| "note-panes".into())
                .flex_1()
                .min_h_0()
                .flex();
            if self.mode != ViewMode::Preview {
                panes = panes.child(
                    div()
                        .debug_selector(|| "source-pane".into())
                        .flex_1()
                        .when(self.mode == ViewMode::Split, |mut pane| {
                            pane.style().flex_grow = Some(self.split_ratio);
                            pane
                        })
                        .min_w_0()
                        .h_full()
                        .child(
                            AnyView::from(editor).cached(StyleRefinement::default().size_full()),
                        ),
                );
            }
            if self.mode == ViewMode::Split {
                let bounds = self.split_bounds.clone();
                panes = panes
                    .relative()
                    .child(
                        canvas(move |rect, _, _| bounds.set(rect), |_, _, _, _| ())
                            .absolute()
                            .size_full(),
                    )
                    .child(
                        div()
                            .id("split-resize")
                            .debug_selector(|| "split-resize".into())
                            .w(px(SPLIT_DIVIDER_WIDTH))
                            .h_full()
                            .flex_shrink_0()
                            .cursor_col_resize()
                            .hover(|s| s.bg(rgb(0x303749)))
                            .flex()
                            .justify_center()
                            .child(div().w(px(1.)).h_full().bg(rgb(LINE)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                    let bounds = this.split_bounds.get();
                                    let available =
                                        f32::from(bounds.size.width) - SPLIT_DIVIDER_WIDTH;
                                    let center = f32::from(bounds.left())
                                        + available * this.split_ratio
                                        + SPLIT_DIVIDER_WIDTH / 2.;
                                    this.split_drag_offset =
                                        Some(f32::from(event.position.x) - center);
                                    window.prevent_default();
                                    cx.stop_propagation();
                                }),
                            ),
                    );
            }
            if self.mode != ViewMode::Source {
                panes = panes.child(
                    div()
                        .debug_selector(|| "preview-pane".into())
                        .flex_1()
                        .when(self.mode == ViewMode::Split, |mut pane| {
                            pane.style().flex_grow = Some(1. - self.split_ratio);
                            pane
                        })
                        .min_w_0()
                        .h_full()
                        .child(
                            AnyView::from(self.preview_pane.clone())
                                .cached(StyleRefinement::default().size_full()),
                        )
                        .relative()
                        .when(self.mode == ViewMode::Split, |pane| {
                            pane.child(
                                div().absolute().top(px(2.)).left(px(4.)).child(
                                    icon_button(
                                        "lock-reading-scroll",
                                        if self.scroll_locked {
                                            ToolbarIcon::Locked
                                        } else {
                                            ToolbarIcon::Unlocked
                                        },
                                        if self.scroll_locked {
                                            "Unlock reading scroll"
                                        } else {
                                            "Lock reading scroll to editor"
                                        }
                                        .into(),
                                        true,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.scroll_locked = !this.scroll_locked;
                                            if !this.scroll_locked {
                                                this.preview_pane
                                                    .update(cx, |pane, _| pane.pending_sync = None);
                                            }
                                            cx.notify();
                                        },
                                    )),
                                ),
                            )
                        }),
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
                                        // The file watcher may have populated contents
                                        // while this window was waiting to be painted.
                                        // Search still needs its own index in that case.
                                        if this.index_revision != this.revision {
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
            main = main.child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(panes)
                    .when(self.outline_open, |row| row.child(self.outline(cx))),
            );
            let rows = self.connections.read(cx).items.len().div_ceil(3).min(3);
            let height = 34. + rows as f32 * 30.;
            main = main.child(
                div().h(px(height)).flex_shrink_0().child(
                    AnyView::from(self.connections.clone())
                        .cached(StyleRefinement::default().size_full()),
                ),
            );
        } else {
            main =
                main.child(
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
                        .child(button("welcome-open", "Open a vault folder").on_click(
                            cx.listener(|this, _, window, cx| this.pick_vault(window, cx)),
                        ))
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
                    this.navigation.width =
                        (f32::from(event.position.x) - SIDEBAR_RAIL_WIDTH).clamp(180., 600.);
                    cx.notify();
                } else if let Some(offset) = this.split_drag_offset
                    && event.pressed_button == Some(MouseButton::Left)
                {
                    let bounds = this.split_bounds.get();
                    let available = f32::from(bounds.size.width) - SPLIT_DIVIDER_WIDTH;
                    if available > 0. {
                        // Keep both panes usable, including in narrow windows.
                        let minimum = (160. / available).min(0.5);
                        this.split_ratio = ((f32::from(event.position.x - bounds.left())
                            - offset
                            - SPLIT_DIVIDER_WIDTH / 2.)
                            / available)
                            .clamp(minimum, 1. - minimum);
                        cx.notify();
                    }
                } else {
                    this.resizing_sidebar = false;
                    this.split_drag_offset = None;
                }
            }))
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _, w, cx| this.visit_history(false, w, cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|this, _, w, cx| this.visit_history(true, w, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.resizing_sidebar = false;
                    this.split_drag_offset = None;
                }),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .debug_selector(|| "sidebar-rail".into())
                            .w(px(SIDEBAR_RAIL_WIDTH))
                            .h_full()
                            .flex_shrink_0()
                            .bg(rgb(0x181c25))
                            .border_r_1()
                            .border_color(rgb(LINE))
                            .flex()
                            .flex_col()
                            .items_center()
                            .pt(px(7.))
                            .child(
                                icon_button(
                                    "toggle-sidebar",
                                    ToolbarIcon::Panel,
                                    if self.navigation.hidden {
                                        "Expand sidebar · Ctrl B"
                                    } else {
                                        "Collapse sidebar · Ctrl B"
                                    }
                                    .into(),
                                    true,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.navigation.hidden = !this.navigation.hidden;
                                        this.sort_menu = None;
                                        this.focus_document(window, cx);
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                icon_button(
                                    "command-palette",
                                    ToolbarIcon::Commands,
                                    "Command palette · Ctrl Shift P".into(),
                                    true,
                                )
                                .on_click(cx.listener(
                                    |this, _, w, cx| this.show_picker(PickerMode::Commands, w, cx),
                                )),
                            )
                            .child(
                                icon_button(
                                    "bookmarks",
                                    ToolbarIcon::Bookmark,
                                    "Bookmarks".into(),
                                    self.vault.is_some(),
                                )
                                .on_click(cx.listener(
                                    |this, _, w, cx| this.show_picker(PickerMode::Bookmarks, w, cx),
                                )),
                            )
                            .child(
                                icon_button(
                                    "daily-note",
                                    ToolbarIcon::Calendar,
                                    "Open today's note · Ctrl Alt D".into(),
                                    self.vault.is_some(),
                                )
                                .on_click(
                                    cx.listener(|this, _, w, cx| this.open_daily_note(w, cx)),
                                ),
                            )
                            .child(div().flex_1())
                            .child(
                                icon_button(
                                    "settings",
                                    ToolbarIcon::Settings,
                                    "Settings · Ctrl ,".into(),
                                    true,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.settings_open = true;
                                        this.focus.focus(window);
                                        cx.notify();
                                    },
                                )),
                            )
                            .pb_2(),
                    )
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
                                .debug_selector(|| "sidebar-resize".into())
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
                    .map(|footer| {
                        #[cfg(debug_assertions)]
                        let footer = footer.child(
                            div().w(px(230.)).h(px(18.)).flex_shrink_0().child(
                                AnyView::from(self.performance.clone())
                                    .cached(StyleRefinement::default().size_full()),
                            ),
                        );
                        footer
                    })
                    .when(self.indexing, |footer| {
                        footer.child(
                            div().w(px(180.)).h(px(18.)).flex_shrink_0().child(
                                AnyView::from(self.indexing_bar.clone())
                                    .cached(StyleRefinement::default().size_full()),
                            ),
                        )
                    })
                    .child(caption(metrics)),
            );
        if !self.completion.candidates.is_empty() {
            root = root.child(self.completion_menu(window, cx));
        }
        if self.picker.is_some() {
            root = root.child(self.picker_panel(cx));
        }
        if self.replace_panel.is_some() {
            root = root.child(self.render_replace(cx));
        }
        if self.mentions.is_some() {
            root = root.child(self.render_mentions(cx));
        }
        if self.comparison.is_some() {
            root = root.child(self.render_comparison(cx));
        }
        if self.properties_panel.is_some() {
            root = root.child(self.render_properties(cx));
        }
        if self.settings_open {
            root = root.child(self.settings_panel(cx));
        }
        if self.sort_menu.is_some() && !self.managing {
            root = root.child(self.render_sort_menu(cx));
        }
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
                Modal::Delete => (
                    "Move to trash?",
                    "Unsaved edits are saved first. Restore this item from the explorer’s right-click menu.",
                    "Move to trash",
                ),
                Modal::Move | Modal::None | Modal::RestoreTrash => unreachable!(),
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
                            .when(self.modal == Modal::Delete, |el| {
                                el.child(
                                    div().text_sm().child(
                                        self.modal_target
                                            .as_ref()
                                            .map(|p| p.display().to_string())
                                            .unwrap_or_default(),
                                    ),
                                )
                            })
                            .when(!matches!(self.modal, Modal::Reload | Modal::Delete), |el| {
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
                                    .child(button("confirm", action).on_click(
                                        cx.listener(|this, _, w, cx| this.submit(w, cx)),
                                    )),
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
                            .child(self.status.clone()),
                    ),
            );
        }
        root
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VaultIndexProgress {
    Reading { completed: usize, total: usize },
    Search(opalglass::search::SyncProgress),
}
impl VaultIndexProgress {
    fn fraction(self) -> f32 {
        let ratio = |done: usize, total: usize| {
            if total == 0 {
                0.
            } else {
                (done as f32 / total as f32).clamp(0., 1.)
            }
        };
        match self {
            Self::Reading { completed, total } => 0.1 * ratio(completed, total),
            Self::Search(opalglass::search::SyncProgress::Notes { completed, total }) => {
                0.1 + 0.8 * ratio(completed, total)
            }
            Self::Search(opalglass::search::SyncProgress::Finalizing) => 0.9,
            Self::Search(opalglass::search::SyncProgress::Complete) => 1.,
        }
    }
    fn detail(self) -> String {
        match self {
            Self::Reading { completed, total } => {
                format!("Reading note links · {completed} / {total}")
            }
            Self::Search(opalglass::search::SyncProgress::Notes { completed, total }) => {
                format!("Indexing notes · {completed} / {total}")
            }
            Self::Search(opalglass::search::SyncProgress::Finalizing) => {
                "Finishing search index…".into()
            }
            Self::Search(opalglass::search::SyncProgress::Complete) => "Search index ready".into(),
        }
    }
}
struct IndexingBar {
    progress: VaultIndexProgress,
}
impl Default for IndexingBar {
    fn default() -> Self {
        Self {
            progress: VaultIndexProgress::Reading {
                completed: 0,
                total: 0,
            },
        }
    }
}
impl Render for IndexingBar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let detail = self.progress.detail();
        div()
            .id("indexing-vault")
            .debug_selector(|| "indexing-vault".into())
            .size_full()
            .flex()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .text_xs()
            .text_color(rgb(MUTED))
            .child("Indexing vault")
            .child(
                div()
                    .w(px(64.))
                    .h(px(4.))
                    .rounded_full()
                    .bg(rgb(LINE))
                    .overflow_hidden()
                    .child(
                        div()
                            .debug_selector(|| "indexing-progress-fill".into())
                            .w(relative(self.progress.fraction()))
                            .h_full()
                            .rounded_full()
                            .bg(rgb(ACCENT)),
                    ),
            )
            .tooltip(move |_, cx| cx.new(|_| PathTooltip(detail.clone())).into())
    }
}

#[cfg(debug_assertions)]
struct PendingPerformance {
    action: &'static str,
    target: String,
    started: Instant,
    ready: Instant,
    frame_started: Option<Instant>,
    phases: Vec<(&'static str, Duration)>,
}

#[cfg(debug_assertions)]
struct PerformanceEntry {
    action: &'static str,
    target: String,
    elapsed: Duration,
    phases: Vec<(&'static str, Duration)>,
}

#[cfg(debug_assertions)]
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
#[cfg(debug_assertions)]
struct PerformanceBar {
    pending: Vec<PendingPerformance>,
    history: std::collections::VecDeque<PerformanceEntry>,
}

#[cfg(debug_assertions)]
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

#[cfg(debug_assertions)]
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
    pending_sync: Option<(ListState, usize, f32)>,
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
        let content = self
            .owner
            .update(cx, |workspace, cx| workspace.preview(cx))
            .unwrap_or_else(|_| div().into_any_element());
        let pending = self.pending_sync.take();
        let entity = cx.entity().downgrade();
        let owner = self.owner.clone();
        div().size_full().relative().child(content).child(
            canvas(
                |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                move |_, hitbox, window, cx| {
                    if let Some((scroll, index, fraction)) = pending
                        && let Some(row) = scroll.bounds_for_item(index)
                    {
                        let offset = row.size.height * fraction;
                        let current = scroll.logical_scroll_top();
                        if current.item_ix == index
                            && (current.offset_in_item - offset).abs() > px(0.5)
                        {
                            // Apply once: the list may clamp near EOF, and retrying that
                            // clamped position every frame would cause a redraw loop.
                            window.defer(cx, move |_, cx| {
                                scroll.scroll_to(ListOffset {
                                    item_ix: index,
                                    offset_in_item: offset,
                                });
                                let _ = entity.update(cx, |_, cx| cx.notify());
                            });
                        }
                    }
                    window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                        if phase.capture() && hitbox.should_handle_scroll(window) {
                            let _ = owner.update(cx, |workspace, cx| {
                                if workspace.scroll_locked
                                    && workspace.mode == ViewMode::Split
                                    && let Some(tab) = workspace.tabs.get(workspace.active)
                                {
                                    tab.editor.update(cx, |editor, cx| {
                                        editor.scroll_by(-event.delta.pixel_delta(px(20.)).y, cx);
                                    });
                                    cx.stop_propagation();
                                }
                            });
                        }
                    });
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
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
                                                            let tag = tag.clone();
                                                            let _ = this.owner.update(
                                                                cx,
                                                                |workspace, cx| {
                                                                    workspace.filter_tag(
                                                                        &tag, window, cx,
                                                                    )
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

include!("workspace_properties.rs");

include!("workspace_comparison.rs");

include!("workspace_attachments.rs");

include!("workspace_mentions.rs");

include!("workspace_replace.rs");
