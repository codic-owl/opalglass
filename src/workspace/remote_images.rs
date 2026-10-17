//! Demand-loaded remote images with a bounded CPU cache and cancellable work.
use super::*;
use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::Weak,
};

type Loaded = Result<Arc<RenderImage>, String>;
type Loader = Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = Loaded> + Send>> + Send + Sync>;
const MAX_ENTRIES: usize = 64;
const MAX_PENDING: usize = 16;
const MAX_ACTIVE: usize = 2;
const CACHE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone)]
pub(super) enum Status {
    Waiting,
    Ready,
    Failed(Arc<str>),
}
pub(super) struct Snapshot {
    pub(super) status: Status,
    pub(super) intrinsic_ratio: Option<f32>,
    // The widget retains this lease, keeping visible images out of eviction.
    pub(super) lease: Arc<()>,
}
enum State {
    Queued,
    Loading,
    Ready(Arc<RenderImage>),
    Failed(Arc<str>),
}
struct Entry {
    state: State,
    lease: Weak<()>,
    used: u64,
    generation: u64,
}

pub(super) struct Cache {
    entries: HashMap<String, Entry>,
    pending: VecDeque<String>,
    active: HashMap<String, Task<()>>,
    reaper: Option<Task<()>>,
    bytes: usize,
    clock: u64,
    generation: u64,
    loader: Loader,
    preview: WeakEntity<PreviewPane>,
}

impl Cache {
    pub(super) fn new(preview: WeakEntity<PreviewPane>, cx: &mut Context<Self>) -> Self {
        cx.on_release(|this, cx| {
            this.active.clear();
            this.reaper = None;
            for (_, entry) in this.entries.drain() {
                if let State::Ready(image) = entry.state {
                    cx.drop_image(image, None);
                }
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
            generation: 0,
            loader: Arc::new(|url| Box::pin(opalglass::images::load(url))),
            preview,
        }
    }

    pub(super) fn request(
        &mut self,
        url: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Snapshot {
        self.clock += 1;
        if !self.entries.contains_key(url) {
            self.prune();
            while self.entries.len() >= MAX_ENTRIES {
                if !self.evict(None, window, cx) {
                    return paused();
                }
            }
            if self.pending.len() >= MAX_PENDING {
                return paused();
            }
            self.generation += 1;
            self.entries.insert(
                url.to_owned(),
                Entry {
                    state: State::Queued,
                    lease: Weak::new(),
                    used: self.clock,
                    generation: self.generation,
                },
            );
            self.pending.push_back(url.to_owned());
        }
        let entry = self.entries.get_mut(url).unwrap();
        entry.used = self.clock;
        let lease = entry.lease.upgrade().unwrap_or_else(|| Arc::new(()));
        entry.lease = Arc::downgrade(&lease);
        let status = match &entry.state {
            State::Ready(_) => Status::Ready,
            State::Failed(message) => Status::Failed(message.clone()),
            _ => Status::Waiting,
        };
        let intrinsic_ratio = match &entry.state {
            State::Ready(image) => {
                let size = image.size(0);
                Some(size.width.0 as f32 / size.height.0 as f32)
            }
            _ => None,
        };
        self.pump(window, cx);
        Snapshot {
            status,
            intrinsic_ratio,
            lease,
        }
    }

    pub(super) fn image(&self, url: &str) -> Option<Arc<RenderImage>> {
        match &self.entries.get(url)?.state {
            State::Ready(image) => Some(image.clone()),
            _ => None,
        }
    }

    pub(super) fn retry(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .entries
            .get(url)
            .is_some_and(|entry| matches!(entry.state, State::Failed(_)))
        {
            self.entries.remove(url);
        }
        // Re-rendering the widget creates its lease and requests the image again.
        let _ = self.preview.update(cx, |_, cx| cx.notify());
        window.refresh();
    }

    fn evict(&mut self, except: Option<&str>, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let oldest = self
            .entries
            .iter()
            .filter(|(url, entry)| {
                Some(url.as_str()) != except
                    && entry.lease.strong_count() == 0
                    && !matches!(entry.state, State::Loading)
            })
            .min_by_key(|(_, entry)| entry.used)
            .map(|(url, _)| url.clone());
        let Some(url) = oldest else {
            return false;
        };
        self.pending.retain(|pending| pending != &url);
        if let Some(entry) = self.entries.remove(&url)
            && let State::Ready(image) = entry.state
        {
            self.bytes -= image.as_bytes(0).map_or(0, <[u8]>::len);
            cx.drop_image(image, Some(window));
        }
        true
    }

    fn prune(&mut self) {
        let obsolete: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, entry)| {
                entry.lease.strong_count() == 0
                    && matches!(entry.state, State::Queued | State::Loading)
            })
            .map(|(url, _)| url.clone())
            .collect();
        for url in obsolete {
            self.active.remove(&url);
            self.entries.remove(&url);
            self.pending.retain(|pending| pending != &url);
        }
    }

    fn pump(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prune();
        while self.active.len() < MAX_ACTIVE {
            let Some(url) = self.pending.pop_front() else {
                break;
            };
            let Some(entry) = self.entries.get_mut(&url) else {
                continue;
            };
            entry.state = State::Loading;
            let generation = entry.generation;
            let loader = self.loader.clone();
            let request_url = url.clone();
            let work = cx
                .background_executor()
                .spawn(async move { loader(request_url).await });
            let handle = window.window_handle();
            let task_url = url.clone();
            let task = cx.spawn(async move |this, cx| {
                let result = work.await;
                let _ = cx.update_window(handle, |_, window, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.publish(&task_url, generation, result, window, cx)
                    });
                });
            });
            self.active.insert(url, task);
        }
        self.reap_when_idle(window, cx);
    }

    fn publish(
        &mut self,
        url: &str,
        generation: u64,
        result: Loaded,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .entries
            .get(url)
            .is_none_or(|e| e.generation != generation)
        {
            return;
        }
        self.active.remove(url);
        let state = match result {
            Ok(image) => {
                let bytes = image.as_bytes(0).map_or(0, <[u8]>::len);
                while self.bytes + bytes > CACHE_BYTES && self.evict(Some(url), window, cx) {}
                if self.bytes + bytes > CACHE_BYTES {
                    State::Failed("Image paused to save memory. Try again after scrolling.".into())
                } else {
                    self.bytes += bytes;
                    State::Ready(image)
                }
            }
            Err(mut message) => {
                if message.len() > 1024 {
                    let mut end = 1024;
                    while !message.is_char_boundary(end) {
                        end -= 1;
                    }
                    message.truncate(end);
                    message.push('…');
                }
                State::Failed(message.into())
            }
        };
        self.entries.get_mut(url).unwrap().state = state;
        self.pump(window, cx);
        // List remeasures visible rows while retaining its logical item/offset
        // anchor. Invalidate this view without resetting measurements above it:
        // width-only images may settle to their decoded intrinsic aspect ratio.
        let _ = self.preview.update(cx, |_, cx| cx.notify());
    }

    fn reap_when_idle(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.reaper.is_some() || self.active.is_empty() && self.pending.is_empty() {
            return;
        }
        let handle = window.window_handle();
        self.reaper = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let Ok(keep) = cx.update_window(handle, |_, window, cx| {
                    this.update(cx, |this, cx| {
                        this.prune();
                        this.pump(window, cx);
                        if this.active.is_empty() && this.pending.is_empty() {
                            this.reaper = None;
                            false
                        } else {
                            true
                        }
                    })
                    .unwrap_or(false)
                }) else {
                    break;
                };
                if !keep {
                    break;
                }
            }
        }));
    }
}

fn paused() -> Snapshot {
    Snapshot {
        status: Status::Failed("More images are waiting. Try again after scrolling.".into()),
        intrinsic_ratio: None,
        lease: Arc::new(()),
    }
}

#[cfg(test)]
#[allow(dead_code)] // Exercised by the separate UI harness, not the binary unit target.
impl Cache {
    pub(super) fn test_loader(&mut self, loader: Loader) {
        self.loader = loader;
    }
    pub(super) fn test_generation(&self, url: &str) -> u64 {
        self.entries[url].generation
    }
    pub(super) fn test_publish(
        &mut self,
        url: &str,
        generation: u64,
        result: Loaded,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.publish(url, generation, result, window, cx);
    }
    pub(super) fn test_stats(&self) -> Stats {
        Stats {
            entries: self.entries.len(),
            active: self.active.len(),
            pending: self.pending.len(),
            ready: self
                .entries
                .values()
                .filter(|e| matches!(e.state, State::Ready(_)))
                .count(),
            failed: self
                .entries
                .values()
                .filter(|e| matches!(e.state, State::Failed(_)))
                .count(),
            bytes: self.bytes,
            reaping: self.reaper.is_some(),
        }
    }
}
#[cfg(test)]
#[allow(dead_code)] // Exercised by the separate UI harness.
pub(super) struct Stats {
    pub(super) entries: usize,
    pub(super) active: usize,
    pub(super) pending: usize,
    pub(super) ready: usize,
    pub(super) failed: usize,
    pub(super) bytes: usize,
    pub(super) reaping: bool,
}
