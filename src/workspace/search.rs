// Vault search scheduling, visible-result snippets and result navigation.
use super::*;

impl Workspace {
    pub(super) fn update_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.search.read(cx).buffer.text();
        if self.search_revision == self.revision && self.search_query == query {
            return;
        }
        let query_changed = self.search_query != query;
        self.search_query = query.to_owned();
        self.search_revision = self.revision;
        if query_changed {
            self.open_search_result = false;
            self.search_selected = 0;
        }
        self.search_cancelled.store(true, Ordering::Relaxed);
        self.search_cancelled = Arc::new(AtomicBool::new(false));
        self._search_task = None;
        self._snippet_task = None;
        self.snippets_loading = false;
        self.snippet_requests.clear();
        self.searching = false;
        if query_changed {
            self.results_scroll = UniformListScrollHandle::new();
        }
        let Some(vault) = self.vault.clone() else {
            return;
        };
        if self.search_query.trim().is_empty() {
            self.search_results.clear();
            return;
        }
        self.searching = true;
        // A saved/externally changed snapshot must finish updating before body
        // results are used. During first indexing, path lookup remains available.
        let tag = query
            .trim()
            .strip_prefix("tag:")
            .or_else(|| query.trim().strip_prefix('#'))
            .filter(|tag| markdown::valid_tag(tag))
            .map(str::to_owned);
        if tag.is_none()
            && self.search_engine.is_some()
            && !self.search_ready()
            && self.search_error.is_none()
        {
            return;
        }
        let engine = self
            .search_ready()
            .then(|| self.search_engine.clone())
            .flatten();
        let query = self.search_query.clone();
        let revision = self.revision;
        let cancelled = self.search_cancelled.clone();
        let task_cancelled = cancelled.clone();
        let started = Instant::now();
        self._search_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            let search_query = query.clone();
            let (results, backend) = cx
                .background_executor()
                .spawn(async move {
                    let started = Instant::now();
                    let result = (|| -> opalglass::search::Result<Vec<SearchRow>> {
                        let ticket = Ticket::from_cancelled(cancelled);
                        ticket.check()?;
                        if let Some(tag) = tag {
                            Ok(vault
                                .notes
                                .iter()
                                .filter(|n| n.has_tag(&tag))
                                .map(|n| SearchRow {
                                    path: n.path.clone(),
                                    title: n.title.clone(),
                                    hit: None,
                                    snippet: None,
                                })
                                .collect())
                        } else if let Some(engine) = engine {
                            let result = engine.search(&search_query, &ticket)?;
                            Ok(result
                                .hits
                                .into_iter()
                                .filter(|hit| vault.contains_path(std::path::Path::new(&hit.path)))
                                .map(|hit| SearchRow {
                                    path: PathBuf::from(&hit.path),
                                    title: hit.title.clone(),
                                    hit: Some(Arc::new(hit)),
                                    snippet: None,
                                })
                                .collect())
                        } else {
                            let needle = search_query.trim().to_lowercase();
                            let mut rows = vault
                                .notes
                                .iter()
                                .filter(|n| {
                                    n.path.to_string_lossy().to_lowercase().contains(&needle)
                                })
                                .map(|n| SearchRow {
                                    path: n.path.clone(),
                                    title: n.title.clone(),
                                    hit: None,
                                    snippet: None,
                                })
                                .collect::<Vec<_>>();
                            rows.sort_by_key(|r| r.title.to_lowercase() != needle);
                            rows.truncate(20);
                            Ok(rows)
                        }
                    })();
                    (result, started.elapsed())
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                if task_cancelled.load(Ordering::Relaxed)
                    || this.revision != revision
                    || this.search.read(cx).buffer.text() != query
                {
                    return;
                }
                this.searching = false;
                match results {
                    Ok(results) => {
                        this.search_results = results;
                        this.search_selected = 0;
                    }
                    Err(error) => {
                        this.search_results.clear();
                        this.message(format!("Search: {error}"), true, cx);
                    }
                }
                this.log_performance_phases(
                    "Search",
                    query.clone(),
                    started,
                    vec![("Search backend", backend)],
                    cx,
                );
                if this.open_search_result
                    && (!this.search_results.is_empty() || this.search_ready())
                {
                    this.open_search_result = false;
                    if this.search.read(cx).focus.is_focused(window) {
                        this.open_first_search_result(window, cx);
                    }
                }
                cx.notify();
            });
        }));
    }
    pub(super) fn queue_search_snippets(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) {
        if self.searching || !self.search_ready() {
            return;
        }
        let Some(engine) = self.search_engine.clone() else {
            return;
        };
        for i in range {
            if self
                .search_results
                .get(i)
                .is_some_and(|r| r.snippet.is_none() && r.hit.is_some())
                && !self.snippet_requests.contains(&i)
            {
                self.snippet_requests.push(i);
            }
        }
        if self.snippets_loading || self.snippet_requests.is_empty() {
            return;
        }
        self.snippets_loading = true;
        let revision = self.revision;
        let query = self.search_query.clone();
        let cancelled = self.search_cancelled.clone();
        let task_cancelled = cancelled.clone();
        let cache = self.snippet_cache.clone();
        self._snippet_task = Some(cx.spawn(async move |this, cx| {
            // UniformList first measures one row, then reports the visible range.
            // Drain after that synchronous render, so both requests form one batch.
            let hits = this
                .update(cx, |this, _| {
                    if task_cancelled.load(Ordering::Relaxed)
                        || this.revision != revision
                        || this.search_query != query
                    {
                        return Vec::new();
                    }
                    std::mem::take(&mut this.snippet_requests)
                        .into_iter()
                        .filter_map(|i| {
                            let row = this.search_results.get(i)?;
                            if row.snippet.is_some() {
                                return None;
                            }
                            Some((i, row.hit.clone()?))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            let started = Instant::now();
            let snippets = cx
                .background_executor()
                .spawn(async move {
                    let ticket = Ticket::from_cancelled(cancelled);
                    let mut cache = cache.lock().map_err(|_| "Snippet cache failed")?;
                    hits.into_iter()
                        .map(|(i, hit)| {
                            ticket.check()?;
                            let snippet = cache
                                .get(&engine, &hit, &ticket)
                                .unwrap_or_else(|_| Snippet::empty("unavailable"));
                            ticket.check()?;
                            Ok((i, snippet))
                        })
                        .collect::<opalglass::search::Result<Vec<_>>>()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if task_cancelled.load(Ordering::Relaxed)
                    || this.revision != revision
                    || this.search_query != query
                {
                    return;
                }
                this.snippets_loading = false;
                match snippets {
                    Ok(snippets) => {
                        for (i, snippet) in snippets {
                            if let Some(row) = this.search_results.get_mut(i) {
                                row.snippet = Some(snippet);
                            }
                        }
                        this.log_performance("Search snippets", query, started, cx);
                    }
                    Err(error) => {
                        // Stop retrying a failed visible batch on every frame.
                        this.search_error = Some(error.to_string());
                        this.message(format!("Could not read search excerpts: {error}"), true, cx);
                    }
                }
                cx.notify();
            });
        }));
    }
    pub(super) fn open_first_search_result(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self
            .search_results
            .get(self.search_selected)
            .map(|r| r.path.clone())
        {
            self.open_note(path, window, cx);
        }
    }
}
