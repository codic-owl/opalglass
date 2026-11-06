//! Source display ranges end at real soft wraps, independently of index chunks.
use super::*;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

const FRAME_CHUNKS: usize = 128;
const CACHE_ROWS: usize = 256;
const CHECKPOINTS: usize = 512;

#[derive(Default)]
pub(super) struct SourceFlow {
    key: Option<(u64, Pixels, u32)>,
    checkpoints: BTreeMap<usize, usize>,
    ranges: BTreeMap<usize, Range<usize>>,
    remaining: usize,
    prepared: Vec<(usize, usize)>,
    requested: Option<(u64, Pixels, u32)>,
    cancel: Option<Arc<AtomicBool>>,
    task: Option<Task<()>>,
    pending_reveal: Option<usize>,
}

impl SourceFlow {
    pub(super) fn begin_frame(&mut self, revision: u64, width: Pixels, font_size: f32) {
        let key = (revision, width, font_size.to_bits());
        if self.key != Some(key) {
            if let Some(cancel) = self.cancel.take() {
                cancel.store(true, Ordering::Relaxed);
            }
            self.task = None;
            self.requested = None;
            self.prepared.clear();
            self.checkpoints.clear();
            self.ranges.clear();
            self.key = Some(key);
        }
        self.remaining = FRAME_CHUNKS;
    }

    pub(super) fn cached(&self, index: usize) -> Option<&Range<usize>> {
        self.ranges.get(&index)
    }

    #[cfg(test)]
    #[allow(dead_code, reason = "Read by the separate native UI test harness")]
    pub(super) fn cache_counts(&self) -> (usize, usize, usize) {
        (self.ranges.len(), self.checkpoints.len(), self.remaining)
    }

    #[cfg(test)]
    #[allow(dead_code, reason = "Read by the separate native UI test harness")]
    pub(super) fn prepared_count(&self) -> usize {
        self.prepared.len()
    }

    fn remember(&mut self, index: usize, range: Range<usize>) {
        if index.is_multiple_of(128) {
            self.checkpoints.insert(index, range.start);
        }
        self.checkpoints.insert(index + 1, range.end);
        while self.checkpoints.len() > CHECKPOINTS {
            let victim = self
                .checkpoints
                .keys()
                .copied()
                .find(|i| !i.is_multiple_of(128));
            self.checkpoints
                .remove(&victim.unwrap_or_else(|| *self.checkpoints.keys().next().unwrap()));
        }
        self.ranges.insert(index, range);
        while self.ranges.len() > CACHE_ROWS {
            let first = *self.ranges.keys().next().unwrap();
            let last = *self.ranges.keys().next_back().unwrap();
            self.ranges
                .remove(&if index.abs_diff(first) > index.abs_diff(last) {
                    first
                } else {
                    last
                });
        }
    }
}

impl Editor {
    pub(super) fn source_display_range(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let raw = self.source_ranges[index].clone();
        if self.live_preview || raw.is_empty() {
            return Some(raw);
        }
        let start = self.buffer.line_start(raw.start);
        let end = self.buffer.line_end(raw.start);
        if start == raw.start && end == raw.end {
            return Some(raw);
        }
        if let Some(range) = self.source_flow.cached(index) {
            return Some(range.clone());
        }
        let first = self.source_ranges.partition_point(|r| r.start < start);
        let local = self
            .source_flow
            .checkpoints
            .range(first..=index)
            .next_back()
            .filter(|(_, byte)| **byte >= start)
            .map(|(index, byte)| (*index, *byte));
        let prepared = self
            .source_flow
            .prepared
            .partition_point(|(row, _)| *row <= index)
            .checked_sub(1)
            .and_then(|at| self.source_flow.prepared.get(at))
            .copied()
            .filter(|(row, byte)| *row >= first && *byte >= start);
        let (mut current, mut byte) = local
            .into_iter()
            .chain(prepared)
            .max_by_key(|(row, _)| *row)
            .unwrap_or((first, start));
        if index - current >= FRAME_CHUNKS {
            if index
                == self
                    .source_ranges
                    .partition_point(|row| row.start <= self.buffer.cursor())
                    .saturating_sub(1)
            {
                self.source_flow.pending_reveal = Some(self.buffer.cursor());
            }
            return None;
        }
        while current <= index && self.source_flow.remaining > 0 {
            self.source_flow.remaining -= 1;
            let target = self.source_ranges[current].end;
            let next = if byte >= target {
                byte
            } else if target == end {
                end
            } else {
                self.source_flow_end(byte, target, end, window)?
            };
            self.source_flow.remember(current, byte..next);
            current += 1;
            byte = next;
        }
        if let Some(range) = self.source_flow.cached(index) {
            Some(range.clone())
        } else {
            if index
                == self
                    .source_ranges
                    .partition_point(|row| row.start <= self.buffer.cursor())
                    .saturating_sub(1)
            {
                self.source_flow.pending_reveal = Some(self.buffer.cursor());
            }
            cx.notify();
            None
        }
    }

    pub(super) fn cancel_source_flow_reveal(&mut self) {
        self.source_flow.pending_reveal = None;
    }

    pub(super) fn start_source_flow_index(&mut self, cx: &mut Context<Self>) {
        if self.live_preview || (self.source_complete && self.source_ranges.len() <= FRAME_CHUNKS) {
            return;
        }
        let Some(key) = self.source_flow.key.filter(|key| key.1 > px(0.)) else {
            return;
        };
        if self.source_flow.requested == Some(key) {
            return;
        }
        self.source_flow.requested = Some(key);
        let cancel = Arc::new(AtomicBool::new(false));
        self.source_flow.cancel = Some(cancel.clone());
        let text = self.buffer.snapshot();
        let system = cx.text_system().clone();
        self.source_flow.task = Some(cx.spawn(async move |this, cx| {
            let prepared = cx
                .background_executor()
                .spawn(async move {
                    source_flow_index::prepare(
                        &text,
                        &system,
                        key.1,
                        f32::from_bits(key.2),
                        &cancel,
                    )
                })
                .await;
            let Some(prepared) = prepared else {
                return;
            };
            let _ = this.update(cx, |editor, cx| {
                if editor.source_flow.key != Some(key) || editor.live_preview {
                    return;
                }
                editor.source_flow.prepared = prepared;
                editor.source_flow.ranges.clear();
                editor.source_flow.checkpoints.clear();
                editor.cached_rows.clear();
                editor.reveal |= editor.source_flow.pending_reveal.take()
                    == Some(editor.buffer.cursor())
                    && editor.pending_scroll.is_none();
                let offset = editor.source_list.logical_scroll_top();
                editor.source_list.splice(
                    0..editor.source_list.item_count(),
                    editor.visibility.rows.len(),
                );
                editor.source_list.scroll_to(offset);
                cx.notify();
            });
        }));
    }

    fn source_flow_end(
        &self,
        start: usize,
        target: usize,
        end: usize,
        window: &Window,
    ) -> Option<usize> {
        // Look ahead by a bounded viewport-sized window, never an entire long line.
        let budget = (f32::from(self.source_width) * 4.).max(512.) as usize;
        let stop_index = self
            .source_ranges
            .partition_point(|range| range.end < target + budget);
        let stop = self
            .source_ranges
            .get(stop_index)
            .or_else(|| self.source_ranges.last())?
            .end
            .min(end);
        let text = &self.buffer.text()[start..stop];
        let line = window
            .text_system()
            .shape_text(
                text.to_owned().into(),
                px(self.font_size),
                &[TextRun {
                    len: text.len(),
                    font: font(MONO_FONT),
                    color: rgb(0xd6d9e3).into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                Some(self.source_width),
                None,
            )
            .ok()?
            .into_iter()
            .next()?;
        // GPUI's glyph wrapping can break inside an extended grapheme (for
        // example a fallback-font ZWJ emoji). Keep that shaping context intact
        // when cutting independently shaped ownership ranges. The existing
        // splitter with a one-byte target enumerates whole graphemes, and this
        // scan covers only the bounded layout window above.
        let graphemes = (!text.is_ascii()).then(|| opalglass::document::chunks(text, 1));
        let boundaries: Vec<_> = line
            .wrap_boundaries
            .iter()
            .map(|boundary| {
                start + line.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index
            })
            .filter(|byte| {
                graphemes.as_ref().is_none_or(|graphemes| {
                    graphemes
                        .binary_search_by_key(&(*byte - start), |range| range.start)
                        .is_ok()
                })
            })
            .collect();
        boundaries
            .iter()
            .copied()
            .take_while(|byte| *byte <= target)
            .last()
            .or_else(|| boundaries.first().copied())
            .or((stop == end).then_some(end))
    }

    pub(super) fn source_index_for(&self, byte: usize) -> usize {
        let original = self
            .source_ranges
            .partition_point(|r| r.start <= byte)
            .saturating_sub(1);
        if self.live_preview {
            return original;
        }
        self.source_flow
            .ranges
            .iter()
            .find(|(_, range)| !range.is_empty() && range.start <= byte && byte <= range.end)
            .map_or(original, |(index, _)| *index)
    }
}

impl Drop for SourceFlow {
    fn drop(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}
