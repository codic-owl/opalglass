//! Reading interactions resolve and edit their original document, never a host by accident.
use super::*;
use reading_context::ReadingContext;
use std::path::Path;

impl Workspace {
    pub(super) fn follow_reading(
        &mut self,
        context: ReadingContext,
        target: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let location = if let Some(number) = target
            .strip_prefix(opalglass::footnotes::TARGET_PREFIX)
            .and_then(|number| number.parse().ok())
        {
            context
                .prepared
                .footnotes
                .target_row(number)
                .map(|row| (row, context.prepared.rows[row].source.start))
        } else if let Some(byte) = target
            .strip_prefix(opalglass::footnotes::BACK_PREFIX)
            .and_then(|byte| byte.parse::<usize>().ok())
        {
            Some((context.prepared.row_at_source(byte), byte))
        } else {
            self.follow_from(&context.path, target, window, cx);
            return;
        };
        let Some((row, byte)) = location else {
            return;
        };
        let revealed = self.reveal_reading_instance(&context, row, cx);
        if context.instance == 0 {
            let Some(tab) = self
                .tabs
                .get_mut(self.active)
                .filter(|tab| context.matches_tab(tab, cx))
            else {
                return;
            };
            tab.editor.update(cx, |editor, cx| editor.jump_to(byte, cx));
            if !revealed {
                tab.reveal_preview_row(row);
            }
            self.preview_pane.update(cx, |_, cx| cx.notify());
        } else if !revealed {
            self.open_reading_source(context, byte, row, window, cx);
        }
    }

    pub(super) fn follow_from(
        &mut self,
        origin: &Path,
        target: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if external_link(&target) {
            cx.open_url(&target);
            return;
        }
        if let Some(tag) = target.strip_prefix(markdown::TAG_TARGET_PREFIX) {
            self.filter_tag(tag, window, cx);
            return;
        }
        let Some(vault) = &self.vault else {
            return;
        };
        if let Ok(path) = opalglass::attachments::resolve(&vault.root, origin, &target) {
            cx.open_with_system(&path);
            return;
        }
        if let Some(path) = vault
            .resolve(origin, &target)
            .or_else(|| vault.resolve_url(origin, &target))
        {
            self.open_note_at(
                path,
                target
                    .split_once('#')
                    .map(|(_, fragment)| fragment.to_owned()),
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

    pub(super) fn toggle_reading_callout(
        &mut self,
        context: ReadingContext,
        id: usize,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) {
        if self.set_reading_instance_fold(&context, id, !collapsed, cx) {
            return;
        }
        if context.instance != 0 {
            return;
        }
        let Some(tab) = self
            .tabs
            .get_mut(self.active)
            .filter(|tab| context.matches_tab(tab, cx))
        else {
            return;
        };
        let offset = tab.preview_scroll.logical_scroll_top();
        let row = tab
            .preview_projection
            .row(tab.composition.projection.host_row(offset.item_ix));
        tab.preview_projection.overrides.insert(id, !collapsed);
        tab.preview_projection
            .rebuild(tab.prepared.rows.len(), &tab.prepared.callouts);
        tab.preview_projection.with_footnotes(
            tab.prepared.rows.len(),
            &tab.prepared.footnotes,
            &tab.prepared.callouts,
        );
        tab.composition.projection = reading_projection::Projection::identity(
            tab.preview_projection.len(tab.prepared.rows.len()),
        );
        tab.composition.key = None;
        tab.preview_scroll.splice(
            0..tab.preview_scroll.item_count(),
            tab.preview_projection.len(tab.prepared.rows.len()),
        );
        tab.preview_scroll.scroll_to(ListOffset {
            item_ix: tab.preview_projection.list_index(row),
            offset_in_item: offset.offset_in_item,
        });
        self.preview_pane.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    pub(super) fn toggle_reading_task(
        &mut self,
        context: ReadingContext,
        range: std::ops::Range<usize>,
        done: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = self
            .tabs
            .iter()
            .position(|tab| context.matches_tab(tab, cx));
        let Some(index) = source else {
            if context.instance == 0 {
                self.message(
                    "The note changed. Try the checkbox again when the preview updates.",
                    false,
                    cx,
                );
            } else {
                self.open_note(context.path, window, cx);
                self.message("Opened source note to edit this task.", false, cx);
            }
            return;
        };
        self.tabs[index].editor.update(cx, |editor, cx| {
            let marker = editor.buffer.text().get(range.clone());
            if marker
                .and_then(opalglass::editing::task_marker)
                .is_some_and(|(current, _)| current == done)
            {
                editor
                    .buffer
                    .replace(range, if done { "[ ]" } else { "[x]" });
                cx.notify();
            }
        });
        cx.notify();
    }
}
