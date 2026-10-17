use super::{Cache, Document, Entry, Input, Key, Loader, SourceToken, State};
use gpui::{Context, Window};
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub(in crate::workspace) struct Stats {
    pub(in crate::workspace) entries: usize,
    pub(in crate::workspace) active: usize,
    pub(in crate::workspace) pending: usize,
    pub(in crate::workspace) ready: usize,
    pub(in crate::workspace) bytes: usize,
}
#[allow(dead_code)]
impl Cache {
    pub(in crate::workspace) fn test_loader(&mut self, loader: Loader) {
        self.loader = loader;
    }
    pub(in crate::workspace) fn test_serial(&self, input: &Input) -> Option<u64> {
        self.entries
            .get(&Key {
                path: input.path.clone(),
                token: input.token,
            })
            .map(|entry| entry.serial)
    }
    pub(in crate::workspace) fn test_prune(&mut self, cx: &mut Context<Self>) {
        self.prune(cx);
    }
    pub(in crate::workspace) fn test_publish(
        &mut self,
        input: &Input,
        serial: u64,
        loaded: Result<Arc<Document>, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.publish(
            &Key {
                path: input.path.clone(),
                token: input.token,
            },
            serial,
            loaded,
            window,
            cx,
        );
    }
    pub(in crate::workspace) fn test_stats(&self) -> Stats {
        Stats {
            entries: self.entries.len(),
            active: self.active.len(),
            pending: self.pending.len(),
            ready: self
                .entries
                .values()
                .filter(|Entry { state, .. }| matches!(state, State::Ready(_)))
                .count(),
            bytes: self.bytes,
        }
    }
}

#[allow(dead_code)]
impl SourceToken {
    pub(in crate::workspace) fn saved_fixture(vault_generation: u64) -> Self {
        Self::Saved {
            vault_generation,
            fingerprint: None,
        }
    }
}
