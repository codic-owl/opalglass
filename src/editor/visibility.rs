//! Compact source-row visibility and callout fold state. Source indices stay physical.
use super::*;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct SourceVisibility {
    pub(super) rows: live::rows::Rows,
    base_key: Option<(u64, usize, usize)>,
    base: Vec<Range<usize>>,
    hidden: Vec<Range<usize>>,
    count: usize,
    overrides: BTreeMap<usize, bool>,
    fold_generation: u64,
    fold_key: Option<(usize, usize, u64)>,
    folded: Vec<Range<usize>>,
    presentation: Option<Presentation>,
}

#[derive(PartialEq, Eq)]
struct Presentation {
    base: (u64, usize, usize),
    revealed: Option<Range<usize>>,
    fold: u64,
}

impl SourceVisibility {
    fn collapsed(&self, callout: &opalglass::callouts::Callout) -> bool {
        callout.collapsed.is_some()
            && self
                .overrides
                .get(&callout.source.start)
                .copied()
                .unwrap_or(callout.collapsed == Some(true))
    }

    fn update(
        &mut self,
        revision: u64,
        source: &str,
        ranges: &[Range<usize>],
        context: Option<&Arc<live::Context>>,
        revealed: Option<&Range<usize>>,
        cursor: Option<usize>,
    ) -> bool {
        let key = (
            revision,
            context.map_or(0, |c| Arc::as_ptr(c) as usize),
            ranges.len(),
        );
        if self.base_key != Some(key) {
            self.base = context.map_or_else(Vec::new, |context| {
                hidden_rows(
                    source,
                    ranges,
                    context.hidden_ranges(),
                    |range| context.footnote_prefix_in(range.clone()),
                    |range| {
                        let continuation =
                            range.start > 0 && source.as_bytes()[range.start - 1] != b'\n';
                        context
                            .project(source, range.clone(), false, false, continuation)
                            .text
                            .trim()
                            .is_empty()
                    },
                )
            });
            self.base_key = Some(key);
            if let Some(context) = context {
                self.overrides.retain(|id, _| {
                    context
                        .callouts()
                        .binary_search_by_key(id, |c| c.callout.source.start)
                        .is_ok()
                });
            }
        }
        if let (Some(context), Some(cursor)) = (context, cursor) {
            let mut ancestor = context.callout_at(cursor);
            while let Some(index) = ancestor {
                let callout = &context.callouts()[index];
                if cursor > callout.title.end && self.collapsed(&callout.callout) {
                    self.overrides.insert(callout.callout.source.start, false);
                    self.fold_generation += 1;
                }
                ancestor = callout.parent;
            }
        }
        let reveal = revealed.map(|range| row_interval(ranges, range));
        let presentation = Presentation {
            base: key,
            revealed: reveal.clone(),
            fold: self.fold_generation,
        };
        if self.presentation.as_ref() == Some(&presentation) {
            return false;
        }
        self.presentation = Some(presentation);
        let mut hidden = Vec::with_capacity(self.base.len());
        for range in &self.base {
            subtract(range.clone(), reveal.as_ref(), &mut hidden);
        }
        let fold_key = (key.1, ranges.len(), self.fold_generation);
        if self.fold_key != Some(fold_key) {
            self.folded.clear();
            if let Some(context) = context {
                for callout in context.callouts() {
                    if self.collapsed(&callout.callout) {
                        // Keep all virtual chunks of the physical title line.
                        let start = ranges.partition_point(|r| r.start <= callout.title.end);
                        let end = ranges.partition_point(|r| r.start < callout.callout.source.end);
                        if start < end {
                            self.folded.push(start..end);
                        }
                    }
                }
            }
            self.fold_key = Some(fold_key);
        }
        hidden.extend(self.folded.iter().cloned());
        merge(&mut hidden);
        if self.count == ranges.len() && self.hidden == hidden {
            return false;
        }
        self.count = ranges.len();
        self.rows = live::rows::Rows::new(self.count, hidden.iter().cloned());
        self.hidden = hidden;
        true
    }
}

impl Editor {
    pub(super) fn collapsed(&self, id: usize) -> bool {
        self.live_context
            .as_ref()
            .filter(|(revision, _)| *revision == self.buffer.revision())
            .and_then(|(_, context)| {
                let callouts = context.callouts();
                callouts
                    .binary_search_by_key(&id, |c| c.callout.source.start)
                    .ok()
                    .map(|index| &callouts[index])
            })
            .is_some_and(|c| self.visibility.collapsed(&c.callout))
    }

    pub(super) fn toggle_callout(&mut self, id: usize, cx: &mut Context<Self>) {
        let Some(callout) = self
            .live_context
            .as_ref()
            .filter(|(revision, _)| *revision == self.buffer.revision())
            .and_then(|(_, context)| {
                let callouts = context.callouts();
                callouts
                    .binary_search_by_key(&id, |c| c.callout.source.start)
                    .ok()
                    .map(|index| &callouts[index])
            })
            .filter(|c| c.callout.collapsed.is_some())
        else {
            return;
        };
        let collapsed = self.visibility.collapsed(&callout.callout);
        if !collapsed
            && self.buffer.cursor() > callout.title.end
            && self.buffer.cursor() < callout.callout.source.end
        {
            self.buffer.move_to(callout.title.start, false);
            self.live_revealed = None;
        }
        self.visibility.overrides.insert(id, !collapsed);
        self.visibility.fold_generation += 1;
        self.cached_rows.clear();
        self.reveal = false;
        self.prepare_source();
        cx.notify();
    }

    pub(super) fn refresh_visibility(&mut self, force: bool, append: Option<usize>) -> bool {
        let offset = self.source_list.logical_scroll_top();
        let physical = if self.visibility.rows.is_empty() {
            offset.item_ix
        } else {
            self.visibility.rows.row(offset.item_ix)
        };
        let context = self
            .live_context
            .as_ref()
            .filter(|(revision, _)| self.live_preview && *revision == self.buffer.revision())
            .map(|(_, context)| context);
        let changed = self.visibility.update(
            self.buffer.revision(),
            self.buffer.text(),
            &self.source_ranges,
            context,
            self.live_revealed.as_ref(),
            Some(self.buffer.cursor()),
        );
        if force || changed {
            if let Some(count) =
                append.filter(|_| self.visibility.rows.len() == self.source_ranges.len())
            {
                self.source_list
                    .splice(count..count, self.source_ranges.len() - count);
            } else {
                self.source_list
                    .splice(0..self.source_list.item_count(), self.visibility.rows.len());
            }
            self.source_list.scroll_to(ListOffset {
                item_ix: self.visibility.rows.list_index(physical),
                offset_in_item: offset.offset_in_item,
            });
        }
        changed
    }

    pub(super) fn invalidate_revealed(&mut self, range: &Range<usize>) {
        let physical = row_interval(&self.source_ranges, range);
        if physical.is_empty() || self.visibility.rows.is_empty() {
            return;
        }
        let start = self.visibility.rows.list_index(physical.start);
        let end = self.visibility.rows.list_index(physical.end - 1) + 1;
        self.source_list.splice(start..end, end - start);
    }
}

fn row_interval(rows: &[Range<usize>], source: &Range<usize>) -> Range<usize> {
    rows.partition_point(|row| row.end < source.start)
        ..rows.partition_point(|row| row.start < source.end)
}

/// Interior rows are hidden in one interval. Only boundary rows need to inspect
/// their (bounded) text, including adjacent hidden constructs on the same line.
fn hidden_rows(
    source: &str,
    rows: &[Range<usize>],
    hidden: &[Range<usize>],
    prefix: impl Fn(&Range<usize>) -> bool,
    projected_empty: impl Fn(&Range<usize>) -> bool,
) -> Vec<Range<usize>> {
    let mut result = Vec::new();
    let last = rows.last().map_or(0, |row| row.end);
    for range in hidden.iter().take_while(|range| range.start <= last) {
        let interval = row_interval(rows, range);
        if interval.is_empty() {
            continue;
        }
        if interval.len() > 2 {
            result.push(interval.start + 1..interval.end - 1);
        }
        for index in [interval.start, interval.end - 1]
            .into_iter()
            .take(if interval.len() == 1 { 1 } else { 2 })
        {
            let row = &rows[index];
            if !prefix(row) && (entirely_hidden(source, row, hidden) || projected_empty(row)) {
                result.push(index..index + 1);
            }
        }
    }
    merge(&mut result);
    result
}

fn entirely_hidden(source: &str, row: &Range<usize>, hidden: &[Range<usize>]) -> bool {
    let first = hidden.partition_point(|range| range.end <= row.start);
    if row.is_empty() {
        return hidden
            .get(first)
            .is_some_and(|range| range.contains(&row.start));
    }
    let mut cursor = row.start;
    let mut covered = false;
    for range in hidden[first..]
        .iter()
        .take_while(|range| range.start < row.end)
    {
        if range.start > cursor && !source[cursor..range.start.min(row.end)].trim().is_empty() {
            return false;
        }
        cursor = cursor.max(range.end.min(row.end));
        covered = true;
    }
    covered && source[cursor..row.end].trim().is_empty()
}

fn subtract(range: Range<usize>, keep: Option<&Range<usize>>, output: &mut Vec<Range<usize>>) {
    let Some(keep) = keep.filter(|keep| keep.start < range.end && keep.end > range.start) else {
        output.push(range);
        return;
    };
    if range.start < keep.start {
        output.push(range.start..keep.start);
    }
    if range.end > keep.end {
        output.push(keep.end..range.end);
    }
}

fn merge(ranges: &mut Vec<Range<usize>>) {
    ranges.sort_unstable_by_key(|range| range.start);
    let mut write = 0;
    for read in 0..ranges.len() {
        if write > 0 && ranges[read].start <= ranges[write - 1].end {
            ranges[write - 1].end = ranges[write - 1].end.max(ranges[read].end);
        } else {
            ranges.swap(write, read);
            write += 1;
        }
    }
    ranges.truncate(write);
}

#[cfg(test)]
#[path = "visibility/tests.rs"]
mod tests;
