use super::*;

pub(super) struct Tab {
    pub(super) path: PathBuf,
    pub(super) baseline: String,
    pub(super) external_changed: bool,
    pub(super) editor: Entity<Editor>,
    pub(super) baseline_normalized: Arc<String>,
    pub(super) dirty_cache: Cell<(u64, bool)>,
    pub(super) prepared: Arc<Prepared>,
    pub(super) prepared_revision: u64,
    pub(super) initial_frame_pending: bool,
    pub(super) pending_anchor: Option<PendingAnchor>,
    pub(super) pending_preview: Option<ListOffset>,
    pub(super) link_style_revision: (u64, u64),
    pub(super) requested_revision: u64,
    pub(super) _prepare: Option<Task<()>>,
    pub(super) _disk_reload: Option<Task<()>>,
    pub(super) prepare_cancelled: Arc<AtomicBool>,
    pub(super) preview_scroll: ListState,
    pub(super) preview_projection: opalglass::callouts::Projection,
    pub(super) composition: reading_composition::State,
    pub(super) connections_cache: Option<ConnectionSnapshot>,
    pub(super) connections_scroll: UniformListScrollHandle,
    pub(super) _subscription: Subscription,
    pub(super) _scroll_subscription: Subscription,
    pub(super) _completion_subscription: Subscription,
    pub(super) _link_subscription: Subscription,
    pub(super) _attachment_subscription: Subscription,
}

pub(super) struct PendingAnchor {
    pub(super) fragment: String,
    pub(super) revision: u64,
    pub(super) cursor: usize,
}

impl Tab {
    pub(super) fn navigate_anchor(&mut self, fragment: &str, cx: &mut App) -> bool {
        let Some(target) = self.prepared.anchors.target(fragment) else {
            return false;
        };
        self.editor
            .update(cx, |editor, cx| editor.jump_to(target.start, cx));
        self.reveal_preview_row(self.prepared.row_at_source(target.start));
        true
    }
    pub(super) fn saved_preview_offset(&self) -> ListOffset {
        let offset = self.preview_scroll.logical_scroll_top();
        ListOffset {
            item_ix: self
                .preview_projection
                .row(self.composition.projection.host_row(offset.item_ix)),
            ..offset
        }
    }
    pub(super) fn restore_preview_offset(&mut self, offset: ListOffset) {
        self.reveal_preview_row(offset.item_ix);
        self.preview_scroll.scroll_to(ListOffset {
            item_ix: self
                .composition
                .projection
                .list_index(self.preview_projection.list_index(offset.item_ix)),
            ..offset
        });
    }
    pub(super) fn install_prepared(&mut self, prepared: Arc<Prepared>, preserve_folds: bool) {
        let offset = self.preview_scroll.logical_scroll_top();
        let byte = self
            .prepared
            .rows
            .get(
                self.preview_projection
                    .row(self.composition.projection.host_row(offset.item_ix)),
            )
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
        if !preserve_folds {
            self.composition =
                reading_composition::State::new(self.preview_projection.len(prepared.rows.len()));
        } else {
            self.composition.projection = reading_projection::Projection::identity(
                self.preview_projection.len(prepared.rows.len()),
            );
            self.composition.key = None;
        }
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
    pub(super) fn reveal_preview_row(&mut self, row: usize) {
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
            self.composition.projection = reading_projection::Projection::identity(
                self.preview_projection.len(self.prepared.rows.len()),
            );
            self.composition.key = None;
            self.preview_scroll.splice(
                0..self.preview_scroll.item_count(),
                self.preview_projection.len(self.prepared.rows.len()),
            );
        }
        self.preview_scroll.scroll_to(ListOffset {
            item_ix: self
                .composition
                .projection
                .list_index(self.preview_projection.list_index(row)),
            offset_in_item: px(0.),
        });
    }
    pub(super) fn cancel_opening_analysis(&mut self) {
        self.pending_anchor = None;
        if !self.prepared.analysis_complete() {
            self.prepare_cancelled.store(true, Ordering::Relaxed);
            self._prepare = None;
            self.requested_revision = u64::MAX;
        }
    }
    pub(super) fn dirty(&self, cx: &App) -> bool {
        let buffer = &self.editor.read(cx).buffer;
        if self.dirty_cache.get().0 != buffer.revision() {
            self.dirty_cache.set((
                buffer.revision(),
                buffer.text() != self.baseline_normalized.as_str(),
            ));
        }
        self.dirty_cache.get().1
    }
    pub(super) fn disk_text(&self, cx: &App) -> String {
        opalglass::document::LineEnding::detect(&self.baseline)
            .encode(self.editor.read(cx).buffer.text())
    }
}

impl Drop for Tab {
    fn drop(&mut self) {
        self.prepare_cancelled.store(true, Ordering::Relaxed);
    }
}
