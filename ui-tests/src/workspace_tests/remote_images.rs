use crate::workspace::{ViewMode, Workspace};
use gpui::{Modifiers, TestAppContext, px, size};
use std::{
    future::poll_fn,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Poll, Waker},
};

const PIXEL: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"2\" height=\"1\"><rect width=\"2\" height=\"1\" fill=\"#2468ac\"/></svg>";

#[derive(Default)]
struct Gate {
    open: AtomicBool,
    wakers: Mutex<Vec<Waker>>,
}
impl Gate {
    async fn wait(&self) {
        poll_fn(|cx| {
            if self.open.load(Ordering::SeqCst) {
                Poll::Ready(())
            } else {
                self.wakers.lock().unwrap().push(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
    fn release(&self) {
        self.open.store(true, Ordering::SeqCst);
        for waker in self.wakers.lock().unwrap().drain(..) {
            waker.wake();
        }
    }
}

#[gpui::test]
fn remote_images_show_loading_errors_retry_and_deduplicate_without_changing_source(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    let text = "![120x60](https://images.example.test/image.png#one)\n\n![120x60](https://images.example.test/image.png#two)\n\n![120x60](https://images.example.test/fail.png)\n";
    std::fs::write(folder.path().join("A.md"), text).unwrap();
    let gate = Arc::new(Gate::default());
    let fail = Arc::new(AtomicBool::new(true));
    let calls = Arc::new(AtomicUsize::new(0));
    let (view, cx) = cx.add_window_view(|w, cx| {
        let workspace = Workspace::new(Some(folder.path().into()), w, cx);
        workspace.remote_images.update(cx, |cache, _| {
            let (gate, fail, calls) = (gate.clone(), fail.clone(), calls.clone());
            cache.test_loader(Arc::new(move |url| {
                let (gate, fail, calls) = (gate.clone(), fail.clone(), calls.clone());
                Box::pin(async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    if url.ends_with("fail.png") && fail.load(Ordering::SeqCst) {
                        return Err("Test image unavailable".into());
                    }
                    gate.wait().await;
                    opalglass::images::prepare(PIXEL)
                })
            }));
        });
        workspace
    });
    view.update_in(cx, |this, w, cx| this.set_mode(ViewMode::Preview, w, cx));
    cx.simulate_resize(size(px(1100.), px(850.)));
    cx.run_until_parked();
    let before = cx.debug_bounds("remote-image-0-0").unwrap();
    assert!(cx.debug_bounds("remote-loading-0-0").is_some());
    assert!(cx.debug_bounds("remote-error-2-0").is_some());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "fragment variants must share one request"
    );
    gate.release();
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let after = cx.debug_bounds("remote-image-0-0").unwrap();
    assert_eq!(
        before, after,
        "completion must preserve explicit image dimensions"
    );
    assert_eq!(after.size, size(px(120.), px(60.)));
    view.read_with(cx, |this, cx| {
        assert!(
            this.remote_images
                .read(cx)
                .image("https://images.example.test/image.png")
                .is_some()
        );
    });
    fail.store(false, Ordering::SeqCst);
    let retry = cx.debug_bounds("retry-image-2-0").unwrap().center();
    cx.simulate_click(retry, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    view.read_with(cx, |this, cx| {
        let cache = this.remote_images.read(cx);
        assert_eq!(cache.test_stats().entries, 2);
        assert!(cache.test_stats().active == 0);
        assert_eq!(cache.test_stats().ready, 2);
        assert_eq!(this.tabs[this.active].editor.read(cx).buffer.text(), text);
    });
    assert_eq!(
        std::fs::read_to_string(folder.path().join("A.md")).unwrap(),
        text
    );
}

#[gpui::test]
fn remote_requests_are_bounded_and_obsolete_work_is_cancelled(cx: &mut TestAppContext) {
    let calls = Arc::new(AtomicUsize::new(0));
    let (view, cx) = cx.add_window_view(|w, cx| {
        let workspace = Workspace::new(None, w, cx);
        workspace.remote_images.update(cx, |cache, _| {
            let calls = calls.clone();
            cache.test_loader(Arc::new(move |_| {
                let calls = calls.clone();
                Box::pin(async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    std::future::pending().await
                })
            }));
        });
        workspace
    });
    let mut leases = view.update_in(cx, |this, w, cx| {
        this.remote_images.update(cx, |cache, cx| {
            (0..100)
                .map(|n| {
                    cache
                        .request(&format!("https://images.example.test/{n}.png"), w, cx)
                        .lease
                })
                .collect::<Vec<_>>()
        })
    });
    cx.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    view.read_with(cx, |this, cx| {
        let cache = this.remote_images.read(cx);
        assert_eq!(cache.test_stats().active, 2);
        assert_eq!(cache.test_stats().pending, 16);
        assert_eq!(cache.test_stats().entries, 18);
    });
    leases.drain(..2);
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(250));
    cx.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    leases.clear();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(250));
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        let cache = this.remote_images.read(cx);
        assert!(cache.test_stats().active == 0);
        assert!(cache.test_stats().pending == 0);
        assert!(cache.test_stats().entries == 0);
        assert!(!cache.test_stats().reaping);
    });
}

#[gpui::test]
fn remote_cache_protects_visible_images_evicts_unused_data_and_ignores_stale_results(
    cx: &mut TestAppContext,
) {
    let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"2600\" height=\"2000\"><rect width=\"2600\" height=\"2000\" fill=\"red\"/></svg>";
    let (view, cx) = cx.add_window_view(|w, cx| {
        let workspace = Workspace::new(None, w, cx);
        workspace.remote_images.update(cx, |cache, _| {
            cache.test_loader(Arc::new(move |_| {
                Box::pin(async move { opalglass::images::prepare(svg) })
            }));
        });
        workspace
    });
    let first = "https://images.example.test/first.png";
    let second = "https://images.example.test/second.png";
    let lease = view.update_in(cx, |this, w, cx| {
        this.remote_images
            .update(cx, |cache, cx| cache.request(first, w, cx).lease)
    });
    cx.run_until_parked();
    let second_lease = view.update_in(cx, |this, w, cx| {
        this.remote_images
            .update(cx, |cache, cx| cache.request(second, w, cx).lease)
    });
    cx.run_until_parked();
    view.read_with(cx, |this, cx| {
        let cache = this.remote_images.read(cx);
        assert!(cache.image(first).is_some());
        assert!(cache.test_stats().failed == 1);
        assert!(cache.test_stats().bytes <= 32 * 1024 * 1024);
    });
    drop(lease);
    let _again = view.update_in(cx, |this, w, cx| {
        this.remote_images.update(cx, |cache, cx| {
            cache.retry(second, w, cx);
            cache.request(second, w, cx).lease
        })
    });
    cx.run_until_parked();
    view.update_in(cx, |this, w, cx| {
        this.remote_images.update(cx, |cache, cx| {
            assert!(cache.image(first).is_none());
            let image = cache.image(second).unwrap();
            let id = image.id;
            let old_generation = cache.test_generation(second) - 1;
            cache.test_publish(second, old_generation, Err("stale result".into()), w, cx);
            assert_eq!(cache.image(second).unwrap().id, id);
            assert!(cache.test_stats().bytes <= 32 * 1024 * 1024);
        })
    });
    drop(second_lease);
}
