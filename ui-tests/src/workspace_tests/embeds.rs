//! Exercise real Reading widgets through the demand-loaded composition and cache.
use super::{TestAppContext, ViewMode, Workspace};
use crate::workspace::{
    embedded_documents::Document, reading_composition::Instance, reading_projection::Row,
};
use gpui::{Entity, ListOffset, Modifiers, VisualTestContext, point, px, size};
use std::{path::Path, sync::Arc};

#[path = "embeds/content.rs"]
mod content;
#[path = "embeds/footnote_navigation.rs"]
mod footnote_navigation;
#[path = "embeds/leases.rs"]
mod leases;
#[path = "embeds/limits.rs"]
mod limits;
#[path = "embeds/navigation.rs"]
mod navigation;
#[path = "embeds/refresh.rs"]
mod refresh;
#[path = "embeds/virtualization.rs"]
mod virtualization;

const PHOTO: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"100\"><rect width=\"200\" height=\"100\" fill=\"#73b4a2\"/></svg>";

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    // Refresh the test selector map, which cached native paint does not retain.
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

fn reading<'a>(
    cx: &'a mut TestAppContext,
    folder: &Path,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.into()), w, cx));
    cx.run_until_parked();
    view.update_in(cx, |this, w, cx| {
        this.open_note("Host.md".into(), w, cx);
        this.set_mode(ViewMode::Preview, w, cx);
    });
    cx.simulate_resize(size(px(1200.), px(1000.)));
    settle(cx);
    (view, cx)
}

fn instances(view: &Entity<Workspace>, cx: &VisualTestContext, label: &str) -> Vec<Instance> {
    let mut instances = view.read_with(cx, |this, _| {
        this.tabs[this.active]
            .composition
            .instances
            .values()
            .filter(|instance| instance.label.as_ref() == label)
            .cloned()
            .collect::<Vec<_>>()
    });
    instances.sort_by_key(|instance| (instance.host, instance.parent, instance.id));
    instances
}

fn document(view: &Entity<Workspace>, cx: &VisualTestContext, id: u64) -> Arc<Document> {
    view.read_with(cx, |this, cx| {
        let instance = &this.tabs[this.active].composition.instances[&id];
        let input = instance.input.as_ref().unwrap();
        this.embedded_documents
            .read(cx)
            .ready(&input.path, input.token)
            .unwrap_or_else(|| {
                panic!(
                    "embed {id} / {} is not ready: {:?}",
                    instance.label, instance.error
                )
            })
    })
}

fn scroll_to(view: &Entity<Workspace>, cx: &mut VisualTestContext, item: usize) {
    view.update(cx, |this, cx| {
        this.tabs[this.active].preview_scroll.scroll_to(ListOffset {
            item_ix: item,
            offset_in_item: px(0.),
        });
        this.preview_pane.update(cx, |_, cx| cx.notify());
    });
    settle(cx);
}

fn show_instance(view: &Entity<Workspace>, cx: &mut VisualTestContext, id: u64) {
    let item = view.read_with(cx, |this, _| {
        let projection = &this.tabs[this.active].composition.projection;
        (0..projection.len())
            .find(|&index| {
                matches!(projection.row(index),
            Some(Row::Header { instance } | Row::Placeholder { instance }) if instance == id)
            })
            .unwrap()
    });
    scroll_to(view, cx, item);
}

fn show_row(view: &Entity<Workspace>, cx: &mut VisualTestContext, id: u64, row: usize) {
    let item = view.read_with(cx, |this, _| {
        let tab = &this.tabs[this.active];
        let logical = tab.composition.instances[&id].projection.list_index(row);
        tab.composition
            .projection
            .find_document_row(id, logical)
            .unwrap_or_else(|| panic!("row {row} is absent from embed {id}"))
    });
    scroll_to(view, cx, item);
}

fn click_selector(cx: &mut VisualTestContext, selector: String) {
    let bounds = cx
        .debug_bounds(Box::leak(selector.into_boxed_str()))
        .unwrap();
    cx.simulate_click(bounds.center(), Modifiers::default());
    settle(cx);
}

fn copy_code(view: &Entity<Workspace>, cx: &mut VisualTestContext, id: u64, expected: &str) {
    let source = document(view, cx, id);
    let row = source
        .prepared
        .rows
        .iter()
        .position(|row| !row.continuation && row.code.as_deref() == Some(expected))
        .unwrap_or_else(|| panic!("missing code {expected:?}"));
    show_row(view, cx, id, row);
    click_selector(cx, format!("embed-{id}-copy-code-{row}"));
    cx.update(|_, cx| assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), expected));
}
