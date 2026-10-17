//! Visible-lease document loading; no vault-wide body retention.
use super::*;
use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::Weak,
};

#[path = "embedded_documents/loader.rs"]
mod loader;
#[path = "embedded_documents/work.rs"]
mod work;

const MAX_ACTIVE: usize = 2;
const MAX_PENDING: usize = 16;
const MAX_ENTRIES: usize = 64;
const CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
const SOURCE_TOO_LARGE: &str =
    "Embedded notes must be 8 MiB or smaller. Open this note in a tab instead.";

type Loaded = Result<Arc<Document>, String>;
type Loader = Arc<
    dyn Fn(Input, Arc<AtomicBool>) -> Pin<Box<dyn Future<Output = Loaded> + Send>> + Send + Sync,
>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum SourceToken {
    Open {
        editor: EntityId,
        revision: u64,
    },
    Saved {
        /// Identity of this vault load, rather than its unrelated update count.
        vault_generation: u64,
        fingerprint: Option<[u8; 32]>,
    },
}
#[derive(Clone)]
pub(super) struct Input {
    pub(super) path: PathBuf,
    pub(super) absolute_path: PathBuf,
    pub(super) token: SourceToken,
    pub(super) open: Option<(Arc<String>, Option<Arc<Prepared>>)>,
}
pub(super) struct Document {
    pub(super) path: PathBuf,
    pub(super) source: Arc<String>,
    pub(super) prepared: Arc<Prepared>,
    pub(super) token: SourceToken,
    pub(super) projection: opalglass::callouts::Projection,
    pub(super) bytes: usize,
}
#[derive(Clone)]
pub(super) enum Status {
    Waiting,
    Ready,
    Failed(Arc<str>),
}
pub(super) struct Snapshot {
    pub(super) status: Status,
    pub(super) document: Option<Arc<Document>>,
    pub(super) lease: Arc<()>,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    path: PathBuf,
    token: SourceToken,
}
enum State {
    Queued,
    Loading,
    Ready(Arc<Document>),
    Failed(Arc<str>),
}
struct Entry {
    input: Option<Input>,
    state: State,
    lease: Weak<()>,
    used: u64,
    serial: u64,
}
struct Active {
    _task: Task<()>,
    cancelled: Arc<AtomicBool>,
}
impl Drop for Active {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

pub(super) struct Cache {
    entries: HashMap<Key, Entry>,
    pending: VecDeque<Key>,
    active: HashMap<Key, Active>,
    reaper: Option<Task<()>>,
    bytes: usize,
    clock: u64,
    serial: u64,
    generation: u64,
    owner: WeakEntity<Workspace>,
    preview: WeakEntity<PreviewPane>,
    loader: Loader,
}
impl Cache {
    pub(super) fn new(
        owner: WeakEntity<Workspace>,
        preview: WeakEntity<PreviewPane>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.on_release(|this, cx| {
            this.active.clear();
            this.reaper = None;
            for (_, entry) in this.entries.drain() {
                retire(entry, cx);
            }
        })
        .detach();
        Self {
            entries: HashMap::new(),
            pending: VecDeque::new(),
            active: HashMap::new(),
            reaper: None,
            bytes: 0,
            clock: 0,
            serial: 0,
            generation: 0,
            owner,
            preview,
            loader: Arc::new(|input, cancelled| Box::pin(loader::load(input, cancelled))),
        }
    }

    pub(super) fn request(
        &mut self,
        input: Input,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Snapshot {
        if input
            .open
            .as_ref()
            .is_some_and(|(source, _)| source.len() > MAX_FILE_BYTES)
        {
            // Reject before queue ownership can pin an arbitrarily large open
            // buffer or preparation; their final release also stays off UI.
            retire(input, cx);
            return Snapshot {
                status: Status::Failed(SOURCE_TOO_LARGE.into()),
                document: None,
                lease: Arc::new(()),
            };
        }
        let key = Key {
            path: input.path.clone(),
            token: input.token,
        };
        // Different revisions of a path never share a ready result. The entry
        // serial also protects against late completion after clear/retry.
        let obsolete: Vec<_> = self
            .entries
            .keys()
            .filter(|old| old.path == key.path && **old != key)
            .cloned()
            .collect();
        for old in obsolete {
            self.remove(&old, cx);
        }
        self.clock = self.clock.wrapping_add(1);
        if !self.entries.contains_key(&key) {
            while self.entries.len() >= MAX_ENTRIES {
                if !self.evict(None, cx) {
                    retire(input, cx);
                    return paused();
                }
            }
            if self.pending.len() >= MAX_PENDING {
                retire(input, cx);
                return paused();
            }
            self.serial = self.serial.wrapping_add(1);
            self.entries.insert(
                key.clone(),
                Entry {
                    input: Some(input),
                    state: State::Queued,
                    lease: Weak::new(),
                    used: self.clock,
                    serial: self.serial,
                },
            );
            self.pending.push_back(key.clone());
            self.notify(cx);
        }
        let entry = self.entries.get_mut(&key).unwrap();
        entry.used = self.clock;
        let lease = entry.lease.upgrade().unwrap_or_else(|| Arc::new(()));
        entry.lease = Arc::downgrade(&lease);
        let (status, document) = match &entry.state {
            State::Ready(document) => (Status::Ready, Some(document.clone())),
            State::Failed(error) => (Status::Failed(error.clone()), None),
            _ => (Status::Waiting, None),
        };
        self.pump(window, cx);
        Snapshot {
            status,
            document,
            lease,
        }
    }
    pub(super) fn ready(
        &self,
        path: &std::path::Path,
        token: SourceToken,
    ) -> Option<Arc<Document>> {
        match &self
            .entries
            .get(&Key {
                path: path.into(),
                token,
            })?
            .state
        {
            State::Ready(document) => Some(document.clone()),
            _ => None,
        }
    }
    pub(super) fn generation(&self) -> u64 {
        self.generation
    }
    pub(super) fn clear(&mut self, cx: &mut Context<Self>) {
        self.active.clear();
        self.pending.clear();
        self.reaper = None;
        for (_, entry) in self.entries.drain() {
            retire(entry, cx);
        }
        self.bytes = 0;
        self.notify(cx);
    }
    pub(super) fn retry(
        &mut self,
        path: &std::path::Path,
        token: SourceToken,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = Key {
            path: path.into(),
            token,
        };
        if self
            .entries
            .get(&key)
            .is_some_and(|entry| matches!(entry.state, State::Failed(_)))
        {
            self.remove(&key, cx);
        }
        self.notify(cx);
        window.refresh();
    }
    fn remove(&mut self, key: &Key, cx: &mut Context<Self>) {
        if self.discard(key, cx) {
            self.notify(cx);
        }
    }
    fn discard(&mut self, key: &Key, cx: &mut Context<Self>) -> bool {
        self.active.remove(key);
        self.pending.retain(|pending| pending != key);
        if let Some(entry) = self.entries.remove(key) {
            if let State::Ready(document) = &entry.state {
                self.bytes = self.bytes.saturating_sub(document.bytes);
            }
            retire(entry, cx);
            true
        } else {
            false
        }
    }
    fn evict(&mut self, except: Option<&Key>, cx: &mut Context<Self>) -> bool {
        let oldest = self
            .entries
            .iter()
            .filter(|(key, entry)| {
                Some(*key) != except
                    && entry.lease.strong_count() == 0
                    && !matches!(entry.state, State::Loading)
            })
            .min_by_key(|(_, entry)| entry.used)
            .map(|(key, _)| key.clone());
        let Some(oldest) = oldest else { return false };
        self.remove(&oldest, cx);
        true
    }
    fn prune(&mut self, cx: &mut Context<Self>) {
        let obsolete: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, entry)| {
                entry.lease.strong_count() == 0
                    && matches!(entry.state, State::Queued | State::Loading)
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in obsolete {
            // An unpainted measurement can start work without a visible lease.
            // Cancelling it must not request another frame, which would measure
            // the same row and restart a slow load indefinitely.
            self.discard(&key, cx);
        }
    }
    fn notify(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        let (owner, preview) = (self.owner.clone(), self.preview.clone());
        cx.defer(move |cx| {
            let _ = owner.update(cx, |_, cx| cx.notify());
            let _ = preview.update(cx, |_, cx| cx.notify());
        });
    }
}
fn retire(value: impl Send + 'static, cx: &App) {
    cx.background_executor()
        .spawn(async move {
            drop(value);
        })
        .detach();
}
fn paused() -> Snapshot {
    Snapshot {
        status: Status::Failed("More embeds are waiting. Retry after scrolling.".into()),
        document: None,
        lease: Arc::new(()),
    }
}

#[cfg(test)]
#[path = "embedded_documents/test_support.rs"]
mod test_support;
