// Indexing progress and development-only performance reporting.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VaultIndexProgress {
    Reading { completed: usize, total: usize },
    Search(opalglass::search::SyncProgress),
}
impl VaultIndexProgress {
    pub(super) fn fraction(self) -> f32 {
        let ratio = |done: usize, total: usize| {
            if total == 0 {
                0.
            } else {
                (done as f32 / total as f32).clamp(0., 1.)
            }
        };
        match self {
            Self::Reading { completed, total } => 0.1 * ratio(completed, total),
            Self::Search(opalglass::search::SyncProgress::Notes { completed, total }) => {
                0.1 + 0.8 * ratio(completed, total)
            }
            Self::Search(opalglass::search::SyncProgress::Finalizing) => 0.9,
            Self::Search(opalglass::search::SyncProgress::Complete) => 1.,
        }
    }
    pub(super) fn detail(self) -> String {
        match self {
            Self::Reading { completed, total } => {
                format!("Reading note links · {completed} / {total}")
            }
            Self::Search(opalglass::search::SyncProgress::Notes { completed, total }) => {
                format!("Indexing notes · {completed} / {total}")
            }
            Self::Search(opalglass::search::SyncProgress::Finalizing) => {
                "Finishing search index…".into()
            }
            Self::Search(opalglass::search::SyncProgress::Complete) => "Search index ready".into(),
        }
    }
}
pub(super) struct IndexingBar {
    pub(super) progress: VaultIndexProgress,
}
impl Default for IndexingBar {
    fn default() -> Self {
        Self {
            progress: VaultIndexProgress::Reading {
                completed: 0,
                total: 0,
            },
        }
    }
}
impl Render for IndexingBar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let detail = self.progress.detail();
        div()
            .id("indexing-vault")
            .debug_selector(|| "indexing-vault".into())
            .size_full()
            .flex()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .text_xs()
            .text_color(rgb(MUTED))
            .child("Indexing vault")
            .child(
                div()
                    .w(px(64.))
                    .h(px(4.))
                    .rounded_full()
                    .bg(rgb(LINE))
                    .overflow_hidden()
                    .child(
                        div()
                            .debug_selector(|| "indexing-progress-fill".into())
                            .w(relative(self.progress.fraction()))
                            .h_full()
                            .rounded_full()
                            .bg(rgb(ACCENT)),
                    ),
            )
            .tooltip(move |_, cx| cx.new(|_| PathTooltip(detail.clone())).into())
    }
}

#[cfg(debug_assertions)]
pub(super) struct PendingPerformance {
    pub(super) action: &'static str,
    pub(super) target: String,
    pub(super) started: Instant,
    pub(super) ready: Instant,
    pub(super) frame_started: Option<Instant>,
    pub(super) phases: Vec<(&'static str, Duration)>,
}

#[cfg(debug_assertions)]
pub(super) struct PerformanceEntry {
    pub(super) action: &'static str,
    pub(super) target: String,
    pub(super) elapsed: Duration,
    pub(super) phases: Vec<(&'static str, Duration)>,
}

#[cfg(debug_assertions)]
impl PerformanceEntry {
    pub(super) fn label(&self) -> String {
        format!(
            "{} · {:.1} ms",
            self.action,
            self.elapsed.as_secs_f64() * 1000.
        )
    }
}

#[derive(Default)]
#[cfg(debug_assertions)]
pub(super) struct PerformanceBar {
    pub(super) pending: Vec<PendingPerformance>,
    pub(super) history: std::collections::VecDeque<PerformanceEntry>,
}

#[cfg(debug_assertions)]
impl PerformanceBar {
    pub(super) fn mark_frame_start(&mut self) {
        let now = Instant::now();
        for sample in &mut self.pending {
            sample.frame_started.get_or_insert(now);
        }
    }
    pub(super) fn log_text(&self) -> String {
        let mut text = String::from(
            "Action → UI paint (excludes GPU presentation and later background analysis).\nLatest 20 actions, newest first. Click to copy.\n",
        );
        for entry in self.history.iter().rev() {
            text.push_str(&format!("\n{} — {}", entry.label(), entry.target));
            for (phase, elapsed) in &entry.phases {
                text.push_str(&format!(
                    "\n  {phase}: {:.1} ms",
                    elapsed.as_secs_f64() * 1000.
                ));
            }
        }
        text
    }
    pub(super) fn tooltip_text(&self) -> String {
        let mut text = String::from("Action → UI paint. Click to copy full timings.\n");
        if let Some(latest) = self.history.back() {
            for (phase, elapsed) in &latest.phases {
                text.push_str(&format!(
                    "\n{phase}: {:.1} ms",
                    elapsed.as_secs_f64() * 1000.
                ));
            }
        }
        text.push_str("\n\nRecent actions (newest first):");
        for entry in self.history.iter().rev() {
            let name = std::path::Path::new(&entry.target)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            text.push_str(&format!("\n{} — {name}", entry.label()));
        }
        text
    }
}

#[cfg(debug_assertions)]
impl Render for PerformanceBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.mark_frame_start();
        let pending = std::mem::take(&mut self.pending);
        let owner = cx.entity().downgrade();
        let tooltip = self.tooltip_text();
        div()
            .id("performance-log")
            .debug_selector(|| "performance-log".into())
            .size_full()
            .text_xs()
            .text_color(rgb(ACCENT))
            .cursor_pointer()
            .truncate()
            .child(
                self.history
                    .back()
                    .map(PerformanceEntry::label)
                    .unwrap_or_else(|| "Performance · —".into()),
            )
            .tooltip(move |_, cx| cx.new(|_| PathTooltip(tooltip.clone())).into())
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(cx.listener(|this, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(this.log_text()));
            }))
            .when(!pending.is_empty(), |el| {
                el.relative().child(
                    canvas(
                        |_, _, _| (),
                        move |_, _, window, cx| {
                            // Capture at paint, before deferring bookkeeping. Updating
                            // this isolated view must not invalidate the note panes.
                            let painted = Instant::now();
                            window.defer(cx, move |_, cx| {
                                let _ = owner.update(cx, |bar, cx| {
                                    for mut sample in pending {
                                        let frame_started =
                                            sample.frame_started.unwrap_or(sample.ready);
                                        sample.phases.push((
                                            "Frame wait",
                                            frame_started.duration_since(sample.ready),
                                        ));
                                        sample.phases.push((
                                            "Render",
                                            painted.duration_since(frame_started),
                                        ));
                                        bar.history.push_back(PerformanceEntry {
                                            action: sample.action,
                                            target: sample.target,
                                            elapsed: painted.duration_since(sample.started),
                                            phases: sample.phases,
                                        });
                                        if bar.history.len() > 20 {
                                            bar.history.pop_front();
                                        }
                                    }
                                    cx.notify();
                                });
                            });
                        },
                    )
                    .absolute()
                    .size(px(1.)),
                )
            })
    }
}
