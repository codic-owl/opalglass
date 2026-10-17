//! Apply source-scoped destinations after synchronous activation or async opening.
use super::super::*;
use reading_context::ReadingContext;

pub(in crate::workspace) enum Destination {
    Fragment(String),
    Reading {
        context: ReadingContext,
        byte: usize,
        row: usize,
    },
}

impl From<&str> for Destination {
    fn from(fragment: &str) -> Self {
        Self::Fragment(fragment.to_owned())
    }
}

impl Workspace {
    pub(in crate::workspace) fn open_reading_source(
        &mut self,
        context: ReadingContext,
        byte: usize,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_note_to(
            context.path.clone(),
            Some(Destination::Reading { context, byte, row }),
            window,
            cx,
        );
    }

    pub(in crate::workspace) fn finish_note_navigation(
        &mut self,
        destination: Option<Destination>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.pending_anchor = None;
        }
        match destination {
            Some(Destination::Fragment(fragment)) => self.navigate_fragment(fragment, cx),
            Some(Destination::Reading { context, byte, row }) => {
                self.navigate_reading_source(context, byte, row, cx);
            }
            None => {}
        }
        self.modal = Modal::None;
        self.inline_parent = None;
        self.explorer_menu = None;
        self.sort_menu = None;
        self.apply_history_position(cx);
        self.focus_document(window, cx);
        cx.notify();
    }

    fn navigate_fragment(&mut self, fragment: String, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        tab.pending_preview = None;
        let editor = tab.editor.read(cx);
        let current = tab.prepared_revision == editor.buffer.revision();
        if current && tab.navigate_anchor(&fragment, cx) {
            self.preview_pane.update(cx, |_, cx| cx.notify());
        } else if !current || !tab.prepared.complete {
            let editor = tab.editor.read(cx);
            tab.pending_anchor = Some(tab::PendingAnchor {
                fragment,
                revision: editor.buffer.revision(),
                cursor: editor.buffer.cursor(),
            });
        } else {
            self.message(
                format!("Link target #{fragment} was not found in this note"),
                false,
                cx,
            );
        }
    }

    fn navigate_reading_source(
        &mut self,
        context: ReadingContext,
        byte: usize,
        row: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self
            .tabs
            .get_mut(self.active)
            .filter(|tab| context.matches_tab(tab, cx))
        else {
            return;
        };
        tab.editor.update(cx, |editor, cx| editor.jump_to(byte, cx));
        if tab.prepared.analysis_complete() {
            tab.pending_preview = None;
            tab.reveal_preview_row(row);
        } else {
            // Opening preparation can omit later rows. Preserve the original
            // physical row until its complete preparation is installed.
            tab.pending_preview = Some(ListOffset {
                item_ix: row,
                offset_in_item: px(0.),
            });
        }
        self.preview_pane.update(cx, |_, cx| cx.notify());
    }
}
