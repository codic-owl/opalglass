//! A redraw replaces visible leases one row at a time.
use super::{CACHE_BYTES, Cache, Status, Workspace, document, input};
use gpui::{AppContext, TestAppContext};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[gpui::test]
fn loading_peers_survive_the_gap_between_old_and_new_frame_leases(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| Workspace::new(None, window, cx));
    let flags = Arc::new(Mutex::new(Vec::new()));
    let cache = view.update(cx, |workspace, cx| {
        let owner = cx.entity().downgrade();
        let preview = workspace.preview_pane.downgrade();
        cx.new(|cx| {
            let mut cache = Cache::new(owner, preview, cx);
            let flags = flags.clone();
            cache.test_loader(Arc::new(move |_, cancelled| {
                flags.lock().unwrap().push(cancelled);
                Box::pin(std::future::pending())
            }));
            cache
        })
    });
    let (a, b) = (input("A.md", 1), input("B.md", 1));
    let (old_a, old_b, serial_a, serial_b) = cx.update(|window, cx| {
        cache.update(cx, |cache, cx| {
            let lease_a = cache.request(a.clone(), window, cx).lease;
            let lease_b = cache.request(b.clone(), window, cx).lease;
            (
                lease_a,
                lease_b,
                cache.test_serial(&a).unwrap(),
                cache.test_serial(&b).unwrap(),
            )
        })
    });
    cx.run_until_parked();
    assert_eq!(flags.lock().unwrap().len(), 2);
    // Both previous paint elements release their captures before the next
    // render starts. Requesting A must not cancel B before B renders again.
    drop((old_a, old_b));
    let (new_a, new_b) = cx.update(|window, cx| {
        cache.update(cx, |cache, cx| {
            let lease_a = cache.request(a.clone(), window, cx).lease;
            assert_eq!(cache.test_serial(&a), Some(serial_a));
            assert_eq!(cache.test_serial(&b), Some(serial_b));
            assert_eq!(cache.test_stats().active, 2);
            assert!(
                flags
                    .lock()
                    .unwrap()
                    .iter()
                    .all(|flag| !flag.load(Ordering::Acquire))
            );
            let lease_b = cache.request(b.clone(), window, cx).lease;
            for (requested, serial) in [(&a, serial_a), (&b, serial_b)] {
                cache.test_publish(
                    requested,
                    serial,
                    Ok(document(requested, "body", 32)),
                    window,
                    cx,
                );
            }
            assert!(cache.ready(&a.path, a.token).is_some());
            assert!(cache.ready(&b.path, b.token).is_some());
            (lease_a, lease_b)
        })
    });
    cx.run_until_parked();
    cache.read_with(cx, |cache, _| {
        assert_eq!(cache.test_stats().ready, 2);
        assert_eq!(cache.test_stats().active, 0);
    });
    drop((new_a, new_b));
    cache.update(cx, |cache, cx| cache.clear(cx));
}

#[gpui::test]
fn unpainted_measurement_completion_is_reused_and_remains_evictable(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| Workspace::new(None, window, cx));
    let starts = Arc::new(AtomicUsize::new(0));
    let cache = view.update(cx, |workspace, cx| {
        let owner = cx.entity().downgrade();
        let preview = workspace.preview_pane.downgrade();
        cx.new(|cx| {
            let mut cache = Cache::new(owner, preview, cx);
            let starts = starts.clone();
            cache.test_loader(Arc::new(move |requested, _| {
                starts.fetch_add(1, Ordering::Relaxed);
                Box::pin(std::future::ready(Ok(document(
                    &requested,
                    "completed while only measured",
                    CACHE_BYTES * 3 / 4,
                ))))
            }));
            cache
        })
    });
    let (a, b) = (input("Measured.md", 1), input("Visible.md", 1));
    cx.update(|window, cx| {
        cache.update(cx, |cache, cx| {
            // List can construct a nearby row for measurement and discard its
            // element without painting or retaining the returned lease.
            let snapshot = cache.request(a.clone(), window, cx);
            assert!(matches!(snapshot.status, Status::Waiting));
        });
    });
    cx.run_until_parked();
    let completed = cache.read_with(cx, |cache, _| cache.ready(&a.path, a.token).unwrap());
    assert_eq!(starts.load(Ordering::Relaxed), 1);
    cx.update(|window, cx| {
        cache.update(cx, |cache, cx| {
            let snapshot = cache.request(a.clone(), window, cx);
            assert!(matches!(snapshot.status, Status::Ready));
            assert!(Arc::ptr_eq(snapshot.document.as_ref().unwrap(), &completed));
            assert_eq!(cache.test_stats().active, 0);
        });
    });
    cx.run_until_parked();
    assert_eq!(starts.load(Ordering::Relaxed), 1);
    let visible = cx.update(|window, cx| {
        cache.update(cx, |cache, cx| cache.request(b.clone(), window, cx).lease)
    });
    cx.run_until_parked();
    cache.read_with(cx, |cache, _| {
        assert!(cache.ready(&a.path, a.token).is_none());
        assert!(cache.ready(&b.path, b.token).is_some());
        assert_eq!(cache.test_stats().ready, 1);
        assert_eq!(cache.test_stats().bytes, CACHE_BYTES * 3 / 4);
        assert!(cache.test_stats().bytes <= CACHE_BYTES);
    });
    assert_eq!(starts.load(Ordering::Relaxed), 2);
    drop(visible);
    cache.update(cx, |cache, cx| cache.clear(cx));
}

#[gpui::test]
fn pruning_an_unpainted_load_is_silent_and_real_demand_can_restart_it(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| Workspace::new(None, window, cx));
    let flags = Arc::new(Mutex::new(Vec::new()));
    let cache = view.update(cx, |workspace, cx| {
        let owner = cx.entity().downgrade();
        let preview = workspace.preview_pane.downgrade();
        cx.new(|cx| {
            let mut cache = Cache::new(owner, preview, cx);
            let flags = flags.clone();
            cache.test_loader(Arc::new(move |_, cancelled| {
                flags.lock().unwrap().push(cancelled);
                Box::pin(std::future::pending())
            }));
            cache
        })
    });
    let measured = input("SlowMeasured.md", 1);
    let old_serial = cx.update(|window, cx| {
        cache.update(cx, |cache, cx| {
            let _snapshot = cache.request(measured.clone(), window, cx);
            cache.test_serial(&measured).unwrap()
        })
    });
    cx.run_until_parked();
    let generation = cache.read_with(cx, |cache, _| cache.generation());
    cache.update(cx, |cache, cx| {
        cache.test_prune(cx);
        assert_eq!(cache.generation(), generation);
        assert_eq!(cache.test_stats().entries, 0);
        assert_eq!(cache.test_stats().active, 0);
        assert_eq!(cache.test_stats().pending, 0);
    });
    assert!(flags.lock().unwrap()[0].load(Ordering::Acquire));
    let visible = cx.update(|window, cx| {
        cache.update(cx, |cache, cx| {
            let snapshot = cache.request(measured.clone(), window, cx);
            assert!(matches!(snapshot.status, Status::Waiting));
            assert_ne!(cache.test_serial(&measured), Some(old_serial));
            assert!(cache.generation() > generation);
            snapshot.lease
        })
    });
    cx.run_until_parked();
    let flags = flags.lock().unwrap();
    assert_eq!(flags.len(), 2);
    assert!(!flags[1].load(Ordering::Acquire));
    drop(flags);
    drop(visible);
    cache.update(cx, |cache, cx| cache.clear(cx));
}
