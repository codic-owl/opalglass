use super::*;

impl Editor {
    pub fn set_live_context(&mut self, context: Arc<live::Context>, revision: u64) {
        if revision != self.buffer.revision()
            || self
                .live_context
                .as_ref()
                .is_some_and(|(r, old)| *r == revision && Arc::ptr_eq(old, &context))
        {
            return;
        }
        self.live_context = Some((revision, context));
        self.live_lines.clear();
        self.cached_rows.clear();
        if self.live_preview && self.source_version == revision {
            self.refresh_visibility(true, None);
        }
    }

    pub(crate) fn link_target_at(&self, index: usize) -> Option<String> {
        if let Some((_, context)) = self
            .live_context
            .as_ref()
            .filter(|(r, _)| *r == self.buffer.revision())
            && let Some(target) = context.link_target_at(index)
        {
            return Some(target.to_owned());
        }
        let start = self.buffer.line_start(index);
        let end = self.buffer.line_end(index);
        let range = if end - start <= 8192 {
            start..end
        } else {
            self.source_ranges
                .get(
                    self.source_ranges
                        .partition_point(|r| r.start <= index)
                        .saturating_sub(1),
                )?
                .clone()
        };
        // The raw line retains automatic URLs inside an inline footnote body.
        // The document projection collapses that body into its reference number.
        let context = self
            .live_context
            .as_ref()
            .filter(|(r, _)| *r == self.buffer.revision())
            .map(|(_, context)| context);
        if context.is_none_or(|context| !context.is_hidden(index)) {
            let raw = live::Line::new(
                &self.buffer.text()[range.clone()],
                false,
                false,
                range.start != start,
            );
            if let Some(target) = projected_target(&raw, &range, index) {
                return Some(target);
            }
        }
        let context = context?;
        let line = context.project(
            self.buffer.text(),
            range.clone(),
            false,
            false,
            range.start != start,
        );
        projected_target(&line, &range, index)
    }
    pub(super) fn prepare_source(&mut self) {
        if self.single_line {
            return;
        }
        let changed = self.source_version != self.buffer.revision();
        if changed {
            if self.syntax_version != self.buffer.revision() {
                self.source_links.clear();
                self.source_syntax = Arc::new(Vec::new());
                self.missing_links.clear();
            }
            self._source_prepare = None;
            if self.source_version == u64::MAX {
                (self.source_ranges, self.source_complete) =
                    opalglass::document::opening_source_rows(self.buffer.text());
            } else {
                self.source_ranges = opalglass::document::source_rows(self.buffer.text());
                self.source_complete = true;
            }
            self.source_version = self.buffer.revision();
            self.find_matches = Arc::new(Vec::new());
            self.cached_rows.clear();
            self.live_lines.clear();
        }
        let previous = self.live_revealed.clone();
        self.update_revealed();
        let visibility_changed = self.refresh_visibility(changed, None);
        if previous != self.live_revealed && !visibility_changed && !changed {
            let offset = self.source_list.logical_scroll_top();
            if let Some(range) = &previous {
                self.invalidate_revealed(range);
            }
            if let Some(range) = self.live_revealed.clone() {
                self.invalidate_revealed(&range);
            }
            self.source_list.scroll_to(offset);
        }
        // Explicit navigation outside the opening viewport must work before the
        // background index arrives (Ctrl+End and heading/block jumps).
        if !self.source_complete
            && self.pending_scroll.is_none()
            && self.buffer.cursor() >= self.source_ranges.last().map_or(0, |r| r.end)
        {
            self.finish_source_index(opalglass::document::source_rows(self.buffer.text()));
            self._source_prepare = None;
        }
        if let Some(pos) = self.pending_scroll
            && (self.source_complete || pos.row < self.source_ranges.len())
        {
            self.source_list.scroll_to(ListOffset {
                item_ix: self.visibility.rows.list_index(pos.row),
                offset_in_item: px(pos.offset),
            });
            self.pending_scroll = None;
        }
        if self.reveal {
            let physical = self
                .source_ranges
                .partition_point(|r| r.start <= self.buffer.cursor())
                .saturating_sub(1);
            let index = self.visibility.rows.list_index(physical);
            let visible = self.visible_rows.iter().any(|r| {
                r.range.start == self.source_ranges[physical].start
                    && r.origin.y >= self.bounds.top()
                    && r.origin.y + r.line_height <= self.bounds.bottom()
            });
            if visible {
                self.source_list.scroll_to_reveal_item(index);
            } else {
                self.source_list.scroll_to(ListOffset {
                    item_ix: index,
                    offset_in_item: px(0.),
                });
            }
            self.reveal = false;
        }
    }

    fn update_revealed(&mut self) {
        if !self.live_preview {
            self.live_revealed = None;
            return;
        }
        let selection = self.buffer.selection();
        let mut revealed =
            self.buffer.line_start(selection.start)..self.buffer.line_end(selection.end) + 1;
        if let Some((_, context)) = self
            .live_context
            .as_ref()
            .filter(|(revision, _)| *revision == self.buffer.revision())
        {
            let range = context.reveal_range(revealed);
            revealed = self.buffer.line_start(range.start)
                ..self
                    .buffer
                    .line_end(range.end.saturating_sub(1).min(self.buffer.text().len()))
                    + 1;
        }
        self.live_revealed = Some(revealed);
    }

    fn finish_source_index(&mut self, ranges: Vec<Range<usize>>) {
        let count = self.source_ranges.len();
        debug_assert_eq!(&ranges[..count], self.source_ranges.as_slice());
        // Identity projections append the new tail without invalidating measured
        // viewport rows. Compact projections retain their physical scroll anchor.
        let append = (self.visibility.rows.len() == count).then_some(count);
        self.source_ranges = ranges;
        self.source_complete = true;
        self.refresh_visibility(false, append);
    }
    pub(super) fn start_source_index(&mut self, revision: u64, cx: &mut Context<Self>) {
        if self.source_complete || self.buffer.revision() != revision {
            return;
        }
        let text = self.buffer.snapshot();
        self._source_prepare = Some(cx.spawn(async move |this, cx| {
            let ranges = cx
                .background_executor()
                .spawn(async move { opalglass::document::source_rows(&text) })
                .await;
            let _ = this.update(cx, |editor, cx| {
                if editor.buffer.revision() != revision || editor.source_complete {
                    return;
                }
                editor.finish_source_index(ranges);
                cx.notify();
            });
        }));
    }
    pub(super) fn projection(&mut self, range: &Range<usize>, revealed: bool) -> Arc<live::Line> {
        if !self.live_preview {
            return Arc::new(live::Line::raw(&self.buffer.text()[range.clone()]));
        }
        let start = self.buffer.line_start(range.start);
        let end = self.buffer.line_end(range.start);
        // Bound even a pathological single-line note. Normal lines are parsed as
        // a unit so inline markup can cross the 256-byte virtual row boundaries.
        let physical = if end - start <= 8192 {
            start..end
        } else {
            range.clone()
        };
        let cached = self
            .live_lines
            .iter()
            .find(|(r, active, _)| *r == physical && *active == revealed)
            .map(|(_, _, line)| line.clone());
        let line = cached.unwrap_or_else(|| {
            let i = self.code_blocks.partition_point(|r| r.end <= range.start);
            let code = self
                .code_blocks
                .get(i)
                .is_some_and(|r| r.contains(&range.start));
            let line = Arc::new(
                if let Some((_, context)) = self
                    .live_context
                    .as_ref()
                    .filter(|(r, _)| *r == self.buffer.revision())
                {
                    context.project(
                        self.buffer.text(),
                        physical.clone(),
                        revealed,
                        code,
                        physical.start != start,
                    )
                } else {
                    live::Line::new(
                        &self.buffer.text()[physical.clone()],
                        revealed,
                        code,
                        physical.start != start,
                    )
                },
            );
            if self.live_lines.len() >= 128 {
                self.live_lines.drain(..64);
            }
            self.live_lines
                .push((physical.clone(), revealed, line.clone()));
            line
        });
        if physical == *range {
            line
        } else {
            Arc::new(line.slice(range.start - physical.start..range.end - physical.start))
        }
    }
}

fn projected_target(line: &live::Line, range: &Range<usize>, index: usize) -> Option<String> {
    line.spans.iter().find_map(|span| {
        let source = range.start + line.source_index(span.range.start)
            ..range.start + line.source_index(span.range.end);
        source
            .contains(&index)
            .then(|| span.style.target.clone())
            .flatten()
    })
}
