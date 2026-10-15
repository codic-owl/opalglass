//! Read-only search exercise in a real GPUI window. No source notes are edited.
#[path = "../src/editor.rs"]
mod editor;
#[path = "../src/theme.rs"]
mod theme;
mod workspace {
    include!("../src/workspace.rs");
    impl Workspace {
        pub fn profile_search(&self, window: &mut Window, cx: &mut Context<Self>) {
            cx.spawn_in(window, async move |this, cx| {
                let startup = Instant::now();
                loop {
                    Timer::after(Duration::from_millis(100)).await;
                    let ready = this
                        .read_with(cx, |this, _| this.search_ready())
                        .unwrap_or(false);
                    if ready {
                        break;
                    }
                    if startup.elapsed() > Duration::from_secs(90) {
                        let _ = this.update(cx, |this, cx| {
                            eprintln!(
                                "Search startup timed out: {}; loading={} note_loading={} indexing={} revision={}/{} first_frame_pending={} content_indexed={} engine={} error={:?}",
                                this.status, this.loading, this.note_loading, this.indexing,
                                this.index_revision, this.revision,
                                this.tabs.get(this.active).is_some_and(|t| t.initial_frame_pending),
                                this.vault.as_ref().is_some_and(|v| v.content_indexed()),
                                this.search_engine.is_some(), this.search_error
                            );
                            cx.quit();
                        });
                        return;
                    }
                }
                let _ = this.update(cx, |this, _| {
                    println!(
                        "Search ready after {:?}; notes={}; persistent={}",
                        startup.elapsed(),
                        this.vault.as_ref().unwrap().notes.len(),
                        this.search_index.as_ref().unwrap().persistent
                    );
                });
                for query in [
                    "Glossary",
                    "glossry",
                    "Saducismus Triumphatus",
                    "divnie light",
                    "compassion",
                    "opalglass-profile-missing-term",
                ] {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.search
                            .update(cx, |editor, cx| editor.set_text(query, cx));
                        this.search.read(cx).focus.focus(window);
                        this.update_search(window, cx);
                        cx.notify();
                    });
                    let started = Instant::now();
                    loop {
                        Timer::after(Duration::from_millis(10)).await;
                        let ready = this
                            .read_with(cx, |this, _| !this.searching && !this.snippets_loading)
                            .unwrap_or(false);
                        if ready {
                            break;
                        }
                        if started.elapsed() > Duration::from_secs(5) {
                            eprintln!("Query timed out: {query}");
                            break;
                        }
                    }
                    // Allow the actual native render of the installed excerpts.
                    Timer::after(Duration::from_millis(100)).await;
                    let _ = this.update(cx, |this, _| {
                        println!(
                            "query={query:?} results={} first={:?} visible_snippets={} error={:?}",
                            this.search_results.len(),
                            this.search_results.first().map(|r| r.title.as_str()),
                            this.search_results
                                .iter()
                                .filter(|r| r.snippet.is_some())
                                .count(),
                            this.search_error
                        );
                    });
                }
                // Scroll a nonempty search to its end to exercise lazy snippets.
                let _ = this.update_in(cx, |this, window, cx| {
                    this.search.update(cx, |e, cx| e.set_text("compassion", cx));
                    this.update_search(window, cx);
                    cx.notify();
                });
                Timer::after(Duration::from_millis(250)).await;
                let _ = this.update(cx, |this, cx| {
                    let last = this.search_results.len().saturating_sub(1);
                    this.results_scroll
                        .scroll_to_item(last, ScrollStrategy::Center);
                    this.sidebar.update(cx, |_, cx| cx.notify());
                });
                Timer::after(Duration::from_millis(250)).await;
                let _ = this.update(cx, |this, cx| {
                    println!(
                        "last result excerpt ready: {}",
                        this.search_results
                            .last()
                            .is_some_and(|r| r.snippet.is_some())
                    );
                    println!("{}", this.performance.read(cx).log_text());
                    cx.quit();
                });
            })
            .detach();
        }
    }
}
use gpui::{prelude::*, *};
fn main() {
    let root = std::env::args_os().nth(1).expect("vault path required");
    Application::new().run(move |cx: &mut App| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1360.), px(860.)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Opalglass search benchmark".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    let workspace = workspace::Workspace::new(Some(root.into()), window, cx);
                    workspace.profile_search(window, cx);
                    workspace
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
