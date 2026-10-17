//! Compose cached transclusions as compact runs; document bodies never enter tab state.
use super::*;
use opalglass::embeds::{self, Ancestry, Budget, Key, Occurrence, Selection, Target};
use reading_composition::{Instance, State};
use reading_context::ReadingContext;
use reading_projection::{Projection, Row};
use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    ops::Range,
};

struct Builder<'a> {
    workspace: &'a Workspace,
    state: &'a State,
    cx: &'a App,
    output: Projection,
    instances: HashMap<u64, Instance>,
    budget: Budget,
}

struct Candidate<'a> {
    occurrence: &'a Occurrence,
    logical: Range<usize>,
    target: Result<Target, embeds::ResolveError>,
}

struct Cursor {
    logical: usize,
    /// Some means an embed consumed the beginning of this row, even at byte zero.
    offset: Option<usize>,
}

impl Workspace {
    pub(super) fn compose_reading(
        &self,
        host: ReadingContext,
        state: &State,
        cx: &App,
    ) -> (Projection, HashMap<u64, Instance>) {
        let source_len = host
            .source
            .as_ref()
            .map(|source| source.len())
            .or_else(|| {
                self.tabs
                    .iter()
                    .find(|tab| host.matches_tab(tab, cx))
                    .map(|tab| tab.editor.read(cx).buffer.text().len())
            })
            .unwrap_or_else(|| host.prepared.rows.last().map_or(0, |row| row.source.end));
        let ancestry = Ancestry::root(Key {
            path: host.path.clone(),
            source: 0..source_len,
        });
        let rows = compose_selection::Rows {
            physical: std::iter::once(0..host.prepared.rows.len()).collect(),
            logical: std::iter::once(0..host.projection.len(host.prepared.rows.len())).collect(),
        };
        let mut builder = Builder {
            workspace: self,
            state,
            cx,
            output: Projection::default(),
            instances: HashMap::new(),
            budget: Budget::default(),
        };
        builder.document(&host, rows, &ancestry, 0, None);
        (builder.output, builder.instances)
    }
}

impl Builder<'_> {
    fn document(
        &mut self,
        context: &ReadingContext,
        selected: compose_selection::Rows,
        ancestry: &Ancestry,
        depth: usize,
        root_host: Option<usize>,
    ) {
        let candidates = self.candidates(context, &selected);
        for range in selected.logical {
            let mut cursor = Cursor {
                logical: range.start,
                offset: None,
            };
            let at = candidates.partition_point(|c| c.logical.start < range.start);
            for candidate in candidates[at..]
                .iter()
                .take_while(|c| c.logical.start < range.end)
            {
                self.before(
                    context,
                    &mut cursor,
                    candidate.logical.start,
                    candidate.occurrence.request.range.start,
                    root_host,
                );
                let host = root_host.unwrap_or(candidate.logical.start);
                let first = self.output.len();
                self.embed(context, candidate, ancestry, depth + 1, host);
                if root_host.is_none() {
                    self.output
                        .cover_host_rows(first, candidate.logical.clone());
                }
                cursor.logical = candidate.logical.end - 1;
                cursor.offset = Some(candidate.occurrence.last_end);
            }
            self.tail(context, &mut cursor, range.end, root_host);
        }
    }

    fn candidates<'a>(
        &self,
        context: &'a ReadingContext,
        selected: &compose_selection::Rows,
    ) -> Vec<Candidate<'a>> {
        let Some(vault) = &self.workspace.vault else {
            return Vec::new();
        };
        let occurrences = &context.prepared.embeds;
        let mut result = Vec::new();
        for physical in &selected.physical {
            let at = occurrences.partition_point(|o| o.rows.end <= physical.start);
            for occurrence in occurrences[at..]
                .iter()
                .take_while(|o| o.rows.start < physical.end)
            {
                if occurrence.rows.start < physical.start || occurrence.rows.end > physical.end {
                    continue;
                }
                let first = context.projection.list_index(occurrence.rows.start);
                let last = context.projection.list_index(occurrence.rows.end - 1);
                if context.projection.row(first) != occurrence.rows.start
                    || context.projection.row(last) != occurrence.rows.end - 1
                {
                    continue;
                }
                if !selected
                    .logical
                    .iter()
                    .any(|range| range.start <= first && last < range.end)
                {
                    continue;
                }
                let target = match embeds::resolve(vault, &context.path, &occurrence.request.target)
                {
                    Ok(None) => continue,
                    Ok(Some(target)) => Ok(target),
                    Err(error) => Err(error),
                };
                result.push(Candidate {
                    occurrence,
                    logical: first..last + 1,
                    target,
                });
            }
        }
        result.sort_unstable_by_key(|c| (c.logical.start, c.occurrence.request.range.start));
        result
    }

    fn before(
        &mut self,
        context: &ReadingContext,
        cursor: &mut Cursor,
        logical: usize,
        byte: usize,
        host: Option<usize>,
    ) {
        if cursor.logical < logical {
            if let Some(offset) = cursor.offset.take() {
                self.fragment(
                    context,
                    cursor.logical,
                    offset..self.text_len(context, cursor.logical),
                    host,
                );
                cursor.logical += 1;
            }
            self.output.rows(
                context.instance,
                cursor.logical..logical,
                host.unwrap_or(cursor.logical),
            );
            cursor.logical = logical;
        }
        self.fragment(
            context,
            logical,
            cursor.offset.take().unwrap_or(0)..byte,
            host,
        );
    }

    fn tail(
        &mut self,
        context: &ReadingContext,
        cursor: &mut Cursor,
        end: usize,
        host: Option<usize>,
    ) {
        if let Some(offset) = cursor.offset.take() {
            self.fragment(
                context,
                cursor.logical,
                offset..self.text_len(context, cursor.logical),
                host,
            );
            cursor.logical += 1;
        }
        self.output.rows(
            context.instance,
            cursor.logical..end,
            host.unwrap_or(cursor.logical),
        );
    }

    fn text_len(&self, context: &ReadingContext, logical: usize) -> usize {
        context.prepared.rows[context.projection.row(logical)]
            .text
            .len()
    }

    fn fragment(
        &mut self,
        context: &ReadingContext,
        logical: usize,
        range: Range<usize>,
        host: Option<usize>,
    ) {
        if range.is_empty() {
            return;
        }
        let part = range.start;
        self.output.push(
            Row::Fragment {
                instance: context.instance,
                index: context.projection.row(logical),
                range,
                part,
            },
            host.unwrap_or(logical),
        );
    }

    fn embed(
        &mut self,
        context: &ReadingContext,
        candidate: &Candidate<'_>,
        ancestry: &Ancestry,
        depth: usize,
        host: usize,
    ) {
        let mut instance = self.instance(context, candidate, depth, host);
        let id = instance.id;
        if instance.error.is_some() {
            instance.input = None;
            self.placeholder(instance);
            return;
        }
        if !self.state.enabled(id) {
            self.placeholder(instance);
            return;
        }
        let input = instance.input.as_ref().unwrap();
        let Some(document) = self
            .workspace
            .embedded_documents
            .read(self.cx)
            .ready(&input.path, input.token)
        else {
            self.placeholder(instance);
            return;
        };
        let target = instance.target.as_ref().unwrap();
        let whole = target
            .fragment
            .as_deref()
            .is_none_or(|fragment| fragment.trim().is_empty());
        let selection = match target
            .clone()
            .select(document.prepared.clone(), document.source.len())
        {
            Ok(selection) => selection,
            Err(_) => {
                instance.error = Some("Heading or block not found".into());
                instance.input = None;
                self.placeholder(instance);
                return;
            }
        };
        let branch = match ancestry.enter(selection.key(), &mut self.budget) {
            Ok(branch) => branch,
            Err(stop) => {
                instance.error = Some(
                    match stop {
                        embeds::Stop::Cycle => "Circular note embed",
                        embeds::Stop::Depth => "Maximum embed depth reached",
                        embeds::Stop::Instances => "Embed expansion limit reached",
                    }
                    .into(),
                );
                instance.input = None;
                self.placeholder(instance);
                return;
            }
        };
        let projection = self.folded_projection(id, &document, &selection);
        let selected = compose_selection::select(&selection, &projection, whole);
        instance.rows = selection.rows;
        instance.selected_source = Some(selection.source);
        instance.projection = projection.clone();
        self.instances.insert(id, instance);
        self.output.push(Row::Header { instance: id }, host);
        let child = ReadingContext::embedded(&document, projection, id);
        self.document(&child, selected, &branch, depth, Some(host));
        self.output.push(Row::Footer { instance: id }, host);
    }

    fn instance(
        &self,
        context: &ReadingContext,
        candidate: &Candidate<'_>,
        depth: usize,
        host: usize,
    ) -> Instance {
        let occurrence = candidate.occurrence;
        let id = instance_id(context, occurrence, candidate.target.as_ref().ok());
        let target = candidate.target.clone().ok();
        let mut input = target
            .as_ref()
            .and_then(|target| self.workspace.embed_input(&target.path, self.cx));
        // Visible widgets reacquire an Open snapshot immediately before requesting
        // the cache. Tab metadata must not pin old source/prepared bodies.
        if let Some(input) = &mut input {
            input.open = None;
        }
        let mut instance = Instance {
            id,
            parent: context.instance,
            host,
            origin: context.path.clone(),
            label: occurrence.request.target.clone(),
            target,
            input,
            projection: opalglass::callouts::Projection::default(),
            rows: 0..0,
            selected_source: None,
            depth,
            error: None,
        };
        if candidate.target.is_err() {
            instance.error = Some("Note not found".into());
        } else if depth > embeds::Limits::default().depth {
            instance.error = Some("Maximum embed depth reached".into());
        } else if instance.input.is_none() {
            instance.error = Some("Could not load this note".into());
        }
        instance
    }

    fn folded_projection(
        &self,
        id: u64,
        document: &embedded_documents::Document,
        selection: &Selection,
    ) -> opalglass::callouts::Projection {
        let mut projection = document.projection.clone();
        let callouts = &document.prepared.callouts;
        if let Some(overrides) = self.state.folds.get(&id) {
            projection.overrides = overrides
                .iter()
                .filter_map(|(&id, &collapsed)| {
                    let index = callouts
                        .binary_search_by_key(&id, |c| c.source.start)
                        .ok()?;
                    let default = callouts[index].collapsed?;
                    (collapsed != default).then_some((id, collapsed))
                })
                .collect();
        }
        // An explicit section/block target inside a folded ancestor must remain
        // reachable when that ancestor's title lies outside the selected view.
        for callout in callouts.iter().filter(|callout| {
            callout.rows.start < selection.rows.start
                && callout.rows.contains(&selection.rows.start)
                && callout.collapsed == Some(true)
        }) {
            projection.overrides.insert(callout.source.start, false);
        }
        if !projection.overrides.is_empty() {
            projection.rebuild(document.prepared.rows.len(), callouts);
            projection.with_footnotes(
                document.prepared.rows.len(),
                &document.prepared.footnotes,
                callouts,
            );
        }
        projection
    }

    fn placeholder(&mut self, instance: Instance) {
        self.output.push(
            Row::Placeholder {
                instance: instance.id,
            },
            instance.host,
        );
        self.instances.insert(instance.id, instance);
    }
}

fn instance_id(context: &ReadingContext, occurrence: &Occurrence, target: Option<&Target>) -> u64 {
    let mut hash = DefaultHasher::new();
    context.instance.hash(&mut hash);
    context.path.hash(&mut hash);
    occurrence.request.image_start.hash(&mut hash);
    context.prepared.rows[occurrence.rows.start]
        .source
        .start
        .hash(&mut hash);
    occurrence.request.target.hash(&mut hash);
    target.map(|target| &target.path).hash(&mut hash);
    hash.finish().max(1)
}
