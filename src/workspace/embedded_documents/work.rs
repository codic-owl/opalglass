use super::*;

impl Cache {
    pub(super) fn pump(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while self.active.len() < MAX_ACTIVE {
            let Some(key) = self.pending.pop_front() else {
                break;
            };
            let Some(entry) = self.entries.get_mut(&key) else {
                continue;
            };
            let Some(input) = entry.input.take() else {
                continue;
            };
            entry.state = State::Loading;
            let serial = entry.serial;
            let cancelled = Arc::new(AtomicBool::new(false));
            let worker_cancelled = cancelled.clone();
            let loader = self.loader.clone();
            let work = cx
                .background_executor()
                .spawn(async move { loader(input, worker_cancelled).await });
            let handle = window.window_handle();
            let task_key = key.clone();
            let task = cx.spawn(async move |this, cx| {
                let loaded = work.await;
                let _ = cx.update_window(handle, |_, window, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.publish(&task_key, serial, loaded, window, cx)
                    });
                });
            });
            self.active.insert(
                key,
                Active {
                    _task: task,
                    cancelled,
                },
            );
        }
        self.reap_when_idle(window, cx);
    }
    pub(super) fn publish(
        &mut self,
        key: &Key,
        serial: u64,
        loaded: Loaded,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .entries
            .get(key)
            .is_none_or(|entry| entry.serial != serial)
        {
            retire(loaded, cx);
            return;
        }
        self.active.remove(key);
        // List layout can measure a nearby row without painting it. A valid
        // completion still belongs in the bounded LRU: dropping it and notifying
        // here makes that same measurement reload the source indefinitely.
        // Offscreen in-flight work is cancelled by the reaper; stale/cancelled
        // results are rejected by the entry serial above.
        let state = match loaded {
            Ok(document) => {
                if document.path != key.path || document.token != key.token {
                    retire(document, cx);
                    State::Failed("The embedded note changed while loading. Retry.".into())
                } else if document.bytes > CACHE_BYTES {
                    retire(document, cx);
                    State::Failed(
                        "This note is too large to embed. Open it in a tab instead.".into(),
                    )
                } else {
                    while self.bytes.saturating_add(document.bytes) > CACHE_BYTES
                        && self.evict(Some(key), cx)
                    {}
                    if self.bytes.saturating_add(document.bytes) > CACHE_BYTES {
                        retire(document, cx);
                        State::Failed("Embed paused to save memory. Retry after scrolling.".into())
                    } else {
                        self.bytes += document.bytes;
                        State::Ready(document)
                    }
                }
            }
            Err(error) => State::Failed(truncate_error(error)),
        };
        if let Some(entry) = self.entries.get_mut(key) {
            entry.state = state;
        }
        self.notify(cx);
        self.pump(window, cx);
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
                        // A redraw releases the previous frame's leases before
                        // acquiring its replacements. Only reap between frames,
                        // rather than cancelling visible peers during request().
                        this.prune(cx);
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
fn truncate_error(mut error: String) -> Arc<str> {
    if error.len() > 1024 {
        let mut end = 1024;
        while !error.is_char_boundary(end) {
            end -= 1;
        }
        error.truncate(end);
        error.push('…');
    }
    error.into()
}
