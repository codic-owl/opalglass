//! Compact projection between source rows and the rows shown in Editing Preview.
use std::ops::Range;

/// Stores hidden intervals rather than one entry for every document row.
#[derive(Clone, Debug, Default)]
pub struct Rows {
    count: usize,
    visible: usize,
    gaps: Vec<Gap>,
}

#[derive(Clone, Debug)]
struct Gap {
    source: Range<usize>,
    visible_before: usize,
    hidden_after: usize,
}

impl Rows {
    /// Clamp, sort and merge hidden intervals. Empty/reversed intervals are ignored.
    pub fn new(count: usize, hidden: impl IntoIterator<Item = Range<usize>>) -> Self {
        let mut ranges: Vec<_> = hidden
            .into_iter()
            .filter_map(|range| {
                let start = range.start.min(count);
                let end = range.end.min(count);
                (start < end).then_some(start..end)
            })
            .collect();
        ranges.sort_unstable_by_key(|range| range.start);
        let mut gaps: Vec<Gap> = Vec::with_capacity(ranges.len());
        for range in ranges {
            if let Some(last) = gaps.last_mut()
                && range.start <= last.source.end
            {
                last.source.end = last.source.end.max(range.end);
            } else {
                gaps.push(Gap {
                    source: range,
                    visible_before: 0,
                    hidden_after: 0,
                });
            }
        }
        gaps.shrink_to_fit();
        let mut hidden = 0;
        for gap in &mut gaps {
            gap.visible_before = gap.source.start - hidden;
            hidden += gap.source.len();
            gap.hidden_after = hidden;
        }
        Self {
            count,
            visible: count - hidden,
            gaps,
        }
    }

    pub fn len(&self) -> usize {
        self.visible
    }

    pub fn is_empty(&self) -> bool {
        self.visible == 0
    }

    /// Resolve a visible list index to its source row, clamping to the last row.
    /// An empty projection returns zero as a sentinel; callers should check `len`.
    pub fn row(&self, list_index: usize) -> usize {
        if self.is_empty() {
            return 0;
        }
        let index = list_index.min(self.visible - 1);
        let gaps = self.gaps.partition_point(|gap| gap.visible_before <= index);
        index
            + gaps
                .checked_sub(1)
                .map_or(0, |gap| self.gaps[gap].hidden_after)
    }

    /// Hidden source rows map to the preceding visible row. Leading hidden rows
    /// map to the first visible row. Out-of-range inputs clamp; empty returns zero.
    pub fn list_index(&self, source_row: usize) -> usize {
        if self.is_empty() {
            return 0;
        }
        let source = source_row.min(self.count - 1);
        let gaps = self.gaps.partition_point(|gap| gap.source.start <= source);
        let Some(gap) = gaps.checked_sub(1).map(|gap| &self.gaps[gap]) else {
            return source;
        };
        if source < gap.source.end {
            gap.visible_before.saturating_sub(1)
        } else {
            source - gap.hidden_after
        }
    }

    pub fn is_visible(&self, source_row: usize) -> bool {
        if source_row >= self.count {
            return false;
        }
        let gaps = self
            .gaps
            .partition_point(|gap| gap.source.start <= source_row);
        gaps.checked_sub(1)
            .is_none_or(|gap| source_row >= self.gaps[gap].source.end)
    }
}

#[cfg(test)]
mod tests;
