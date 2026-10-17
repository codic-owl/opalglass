//! Composition invalidation, source snapshots and source-scoped navigation.
use super::*;
use embedded_documents::{Input, SourceToken};
use reading_context::ReadingContext;
use std::{
    hash::{Hash, Hasher},
    path::Path,
};

impl Workspace {
    pub(super) fn embed_input(&self, path: &Path, cx: &App) -> Option<Input> {
        let vault = self.vault.as_ref()?;
        if !vault.contains_path(path) {
            return None;
        }
        let absolute_path = vault.root.join(path);
        if let Some(tab) = self.tabs.iter().find(|tab| tab.path == path) {
            let editor = tab.editor.read(cx);
            let revision = editor.buffer.revision();
            Some(Input {
                path: path.to_owned(),
                absolute_path,
                token: SourceToken::Open {
                    editor: tab.editor.entity_id(),
                    revision,
                },
                open: Some((
                    editor.buffer.snapshot(),
                    (tab.prepared_revision == revision && tab.prepared.analysis_complete())
                        .then(|| tab.prepared.clone()),
                )),
            })
        } else {
            Some(Input {
                path: path.to_owned(),
                absolute_path,
                token: SourceToken::Saved {
                    vault_generation: self.load_generation,
                    fingerprint: vault.note(path)?.content_fingerprint(),
                },
                open: None,
            })
        }
    }

    fn open_embed_versions(&self, cx: &App) -> u64 {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        for tab in &self.tabs {
            (
                &tab.path,
                tab.editor.entity_id(),
                tab.editor.read(cx).buffer.revision(),
            )
                .hash(&mut hash);
        }
        hash.finish()
    }

    pub(super) fn invalidate_embed_sources(&self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        if tab.prepared.embeds.is_empty() {
            return;
        }
        if tab
            .composition
            .key
            .is_some_and(|key| key.3 != self.revision || key.5 != self.open_embed_versions(cx))
        {
            self.preview_pane.update(cx, |_, cx| cx.notify());
        }
    }

    pub(super) fn rebuild_reading_composition(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let mut folds = std::collections::hash_map::DefaultHasher::new();
        for entry in &tab.preview_projection.overrides {
            entry.hash(&mut folds);
        }
        let key = (
            Arc::as_ptr(&tab.prepared) as usize,
            folds.finish(),
            self.embedded_documents.read(cx).generation(),
            self.revision,
            tab.composition.generation,
            self.open_embed_versions(cx),
        );
        if tab.composition.key == Some(key) {
            return;
        }
        let context = ReadingContext::host(tab);
        let scroll = tab.preview_scroll.clone();
        let offset = scroll.logical_scroll_top();
        let mut state = std::mem::take(&mut self.tabs[self.active].composition);
        let mut anchor = state
            .projection
            .anchor(offset.item_ix, offset.offset_in_item.into());
        let source_row = anchor.as_ref().and_then(|anchor| match anchor.row {
            reading_projection::Row::Document { instance, index } => {
                let projection = if instance == 0 {
                    &context.projection
                } else {
                    &state.instances.get(&instance)?.projection
                };
                Some((instance, projection.row(index)))
            }
            _ => None,
        });
        let (projection, instances) = self.compose_reading(context, &state, cx);
        // Logical row indices shift when a callout above the viewport folds.
        // Preserve the actual document row, rather than an unrelated row that
        // inherited its old logical index.
        let mut source_survives = true;
        if let Some((instance, physical)) = source_row {
            let projection = if instance == 0 {
                Some(&self.tabs[self.active].preview_projection)
            } else {
                instances
                    .get(&instance)
                    .map(|metadata| &metadata.projection)
            };
            source_survives = projection.is_some_and(|projection| {
                let index = projection.list_index(physical);
                if projection.row(index) != physical {
                    return false;
                }
                if let Some(anchor) = &mut anchor {
                    anchor.row = reading_projection::Row::Document { instance, index };
                }
                true
            });
        }
        state.projection = projection;
        state.instances = instances;
        state.key = Some(key);
        scroll.splice(0..scroll.item_count(), state.projection.len());
        let index = anchor.as_ref().map_or(0, |anchor| {
            if source_survives {
                state.projection.restore(anchor)
            } else {
                state.projection.list_index(anchor.host)
            }
        });
        scroll.scroll_to(ListOffset {
            item_ix: index,
            offset_in_item: anchor
                .as_ref()
                .filter(|_| source_survives)
                .filter(|anchor| state.projection.row(index).as_ref() == Some(&anchor.row))
                .map_or(px(0.), |anchor| px(anchor.offset)),
        });
        self.tabs[self.active].composition = state;
    }

    pub(super) fn reading_context_for(&self, instance: u64, cx: &App) -> Option<ReadingContext> {
        let tab = self.tabs.get(self.active)?;
        if instance == 0 {
            return Some(ReadingContext::host(tab));
        }
        let metadata = tab.composition.instances.get(&instance)?;
        let input = metadata.input.as_ref()?;
        let document = self
            .embedded_documents
            .read(cx)
            .ready(&input.path, input.token)?;
        Some(ReadingContext::embedded(
            &document,
            metadata.projection.clone(),
            instance,
        ))
    }

    pub(super) fn demand_reading_instance(
        &mut self,
        editor: EntityId,
        instance: u64,
        token: Option<SourceToken>,
        cx: &mut Context<Self>,
    ) -> Option<(Vec<Arc<()>>, Vec<Input>)> {
        let tab = self
            .tabs
            .get(self.active)
            .filter(|tab| tab.editor.entity_id() == editor)?;
        if !tab.composition.instances.contains_key(&instance)
            || tab.composition.token(instance) != token
        {
            return None;
        }
        let mut chain = Vec::new();
        let mut id = instance;
        // The depth-limit placeholder is one level beyond the last expanded
        // document and must still retain all of its visible ancestors.
        while id != 0 && chain.len() <= opalglass::embeds::Limits::default().depth {
            let metadata = tab.composition.instances.get(&id)?;
            let input = if let Some(stored) = &metadata.input {
                let input = self.embed_input(&stored.path, cx)?;
                if input.token != stored.token {
                    return None;
                }
                Some(input)
            } else {
                None
            };
            chain.push((id, input));
            id = metadata.parent;
        }
        if id != 0 {
            return None;
        }
        chain.reverse();
        let state = &mut self.tabs[self.active].composition;
        let (mut leases, mut inputs, mut changed) = (Vec::new(), Vec::new(), false);
        for (id, input) in chain {
            let (lease, fresh) = state.demand(id)?;
            leases.push(lease);
            inputs.extend(input);
            changed |= fresh;
        }
        if changed {
            self.preview_pane.update(cx, |_, cx| cx.notify());
        }
        Some((leases, inputs))
    }

    pub(super) fn reveal_reading_instance(
        &mut self,
        context: &ReadingContext,
        row: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        if context.instance == 0 {
            return false;
        }
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return false;
        };
        if tab.composition.token(context.instance) != context.token {
            return false;
        }
        for callout in &context.prepared.callouts {
            if callout.rows.contains(&row) && row > callout.rows.start {
                tab.composition
                    .folds
                    .entry(context.instance)
                    .or_default()
                    .insert(callout.source.start, false);
            }
        }
        tab.composition.generation = tab.composition.generation.wrapping_add(1);
        self.rebuild_reading_composition(cx);
        let tab = &mut self.tabs[self.active];
        let Some(metadata) = tab.composition.instances.get(&context.instance) else {
            return false;
        };
        let logical = metadata.projection.list_index(row);
        let Some(index) = tab
            .composition
            .projection
            .find_document_row(context.instance, logical)
        else {
            return false;
        };
        tab.preview_scroll.scroll_to(ListOffset {
            item_ix: index,
            offset_in_item: px(0.),
        });
        self.preview_pane.update(cx, |_, cx| cx.notify());
        true
    }

    pub(super) fn set_reading_instance_fold(
        &mut self,
        context: &ReadingContext,
        id: usize,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if context.instance == 0 {
            return false;
        }
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return true;
        };
        if tab.composition.token(context.instance) != context.token {
            return true;
        }
        tab.composition
            .folds
            .entry(context.instance)
            .or_default()
            .insert(id, collapsed);
        tab.composition.generation = tab.composition.generation.wrapping_add(1);
        self.rebuild_reading_composition(cx);
        self.preview_pane.update(cx, |_, cx| cx.notify());
        cx.notify();
        true
    }
}
