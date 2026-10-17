use crate::workspace::Workspace;
use crate::workspace::embedded_documents::{Cache, Document, Input, SourceToken, Status};
use gpui::{AppContext, TestAppContext};
use opalglass::prepared::Prepared;
use std::{
    path::Path,
    sync::{Arc, Mutex, atomic::Ordering},
};

const MAX_ACTIVE: usize = 2;
const MAX_PENDING: usize = 16;
const MAX_ENTRIES: usize = 64;
const CACHE_BYTES: usize = 64 * 1024 * 1024;

#[path = "embedded_documents/lifecycle.rs"]
mod lifecycle;

fn input(name: &str, generation: u64) -> Input {
    Input {
        path: name.into(),
        absolute_path: name.into(),
        token: SourceToken::saved_fixture(generation),
        open: None,
    }
}
fn document(input: &Input, text: &str, bytes: usize) -> Arc<Document> {
    Arc::new(Document {
        path: input.path.clone(),
        source: Arc::new(text.into()),
        prepared: Arc::new(Prepared::new(text)),
        token: input.token,
        projection: opalglass::callouts::Projection::default(),
        bytes,
    })
}

#[gpui::test]
fn visible_jobs_are_deduplicated_bounded_and_cancelled_after_leases_drop(cx: &mut TestAppContext) {
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
    let leases = cx.update(|window, cx| {
        cache.update(cx, |cache, cx| {
            let mut leases = Vec::new();
            for index in 0..100 {
                let requested = input(&format!("{index}.md"), 1);
                let snapshot = cache.request(requested.clone(), window, cx);
                if index == 0 {
                    let duplicate = cache.request(requested, window, cx);
                    assert!(Arc::ptr_eq(&snapshot.lease, &duplicate.lease));
                }
                leases.push(snapshot.lease);
            }
            assert_eq!(cache.test_stats().active, MAX_ACTIVE);
            assert_eq!(cache.test_stats().pending, MAX_PENDING);
            assert_eq!(cache.test_stats().entries, MAX_ACTIVE + MAX_PENDING);
            assert!(cache.test_stats().entries <= MAX_ENTRIES);
            leases
        })
    });
    cx.run_until_parked();
    assert_eq!(flags.lock().unwrap().len(), MAX_ACTIVE);
    drop(leases);
    cache.update(cx, |cache, cx| {
        cache.test_prune(cx);
        assert_eq!(cache.test_stats().active, 0);
        assert_eq!(cache.test_stats().pending, 0);
        assert_eq!(cache.test_stats().entries, 0);
        cache.clear(cx);
    });
    assert!(
        flags
            .lock()
            .unwrap()
            .iter()
            .all(|flag| flag.load(Ordering::Acquire))
    );
}

#[gpui::test]
fn open_snapshots_reuse_current_preparation_and_do_not_read_the_saved_file(
    cx: &mut TestAppContext,
) {
    let (view, cx) = cx.add_window_view(|window, cx| Workspace::new(None, window, cx));
    let source = Arc::new("Unsaved **é**\n\n> [!note]- Fold\n> hidden\n".to_owned());
    let prepared = Arc::new(Prepared::new(&source));
    let (cache, token) = view.update(cx, |workspace, cx| {
        let token = SourceToken::Open {
            editor: cx.entity().entity_id(),
            revision: 900,
        };
        let owner = cx.entity().downgrade();
        let preview = workspace.preview_pane.downgrade();
        (cx.new(|cx| Cache::new(owner, preview, cx)), token)
    });
    let lease = cx.update(|window, cx| {
        cache.update(cx, |cache, cx| {
            let snapshot = cache.request(
                Input {
                    path: "Open.md".into(),
                    absolute_path: "/missing/never-read.md".into(),
                    token,
                    open: Some((source.clone(), Some(prepared.clone()))),
                },
                window,
                cx,
            );
            assert!(matches!(snapshot.status, Status::Waiting));
            snapshot.lease
        })
    });
    cx.run_until_parked();
    cache.read_with(cx, |cache, _| {
        let loaded = cache.ready(Path::new("Open.md"), token).unwrap();
        assert!(Arc::ptr_eq(&loaded.source, &source));
        assert!(Arc::ptr_eq(&loaded.prepared, &prepared));
        assert!(loaded.projection.len(loaded.prepared.rows.len()) < loaded.prepared.rows.len());
        assert!(loaded.bytes >= source.len());
        assert!(cache.test_stats().bytes <= CACHE_BYTES);
        assert_eq!(Arc::strong_count(&lease), 1);
    });
}

#[gpui::test]
fn stale_completions_after_clear_or_token_replacement_never_publish(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| Workspace::new(None, window, cx));
    let cache = view.update(cx, |workspace, cx| {
        let owner = cx.entity().downgrade();
        let preview = workspace.preview_pane.downgrade();
        cx.new(|cx| {
            let mut cache = Cache::new(owner, preview, cx);
            cache.test_loader(Arc::new(|_, _| Box::pin(std::future::pending())));
            cache
        })
    });
    cx.update(|window, cx| {
        cache.update(cx, |cache, cx| {
            let old = input("A.md", 1);
            let _old_lease = cache.request(old.clone(), window, cx).lease;
            let old_serial = cache.test_serial(&old).unwrap();
            cache.clear(cx);
            let _new_lease = cache.request(old.clone(), window, cx).lease;
            cache.test_publish(
                &old,
                old_serial,
                Ok(document(&old, "stale", 10)),
                window,
                cx,
            );
            assert!(cache.ready(&old.path, old.token).is_none());
            let newer = input("A.md", 2);
            let _newer_lease = cache.request(newer.clone(), window, cx).lease;
            assert!(cache.test_serial(&old).is_none());
            cache.test_publish(
                &old,
                old_serial,
                Ok(document(&old, "stale again", 10)),
                window,
                cx,
            );
            let serial = cache.test_serial(&newer).unwrap();
            cache.test_publish(
                &newer,
                serial,
                Ok(document(&newer, "current", 10)),
                window,
                cx,
            );
            assert_eq!(
                cache
                    .ready(&newer.path, newer.token)
                    .unwrap()
                    .source
                    .as_str(),
                "current"
            );
            cache.clear(cx);
        })
    });
}

#[gpui::test]
fn pinned_entries_block_budget_admission_until_retry_after_scrolling(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| Workspace::new(None, window, cx));
    let cache = view.update(cx, |workspace, cx| {
        let owner = cx.entity().downgrade();
        let preview = workspace.preview_pane.downgrade();
        cx.new(|cx| {
            let mut cache = Cache::new(owner, preview, cx);
            cache.test_loader(Arc::new(|_, _| Box::pin(std::future::pending())));
            cache
        })
    });
    cx.update(|window, cx| {
        cache.update(cx, |cache, cx| {
            let (a, b) = (input("A.md", 1), input("B.md", 1));
            let lease_a = cache.request(a.clone(), window, cx).lease;
            let _lease_b = cache.request(b.clone(), window, cx).lease;
            for request in [&a, &b] {
                let serial = cache.test_serial(request).unwrap();
                cache.test_publish(
                    request,
                    serial,
                    Ok(document(request, "body", CACHE_BYTES * 3 / 4)),
                    window,
                    cx,
                );
            }
            assert!(cache.ready(&a.path, a.token).is_some());
            assert!(matches!(
                cache.request(b.clone(), window, cx).status,
                Status::Failed(_)
            ));
            assert!(cache.test_stats().bytes <= CACHE_BYTES);
            drop(lease_a);
            cache.retry(&b.path, b.token, window, cx);
            let _retry_lease = cache.request(b.clone(), window, cx).lease;
            let serial = cache.test_serial(&b).unwrap();
            cache.test_publish(
                &b,
                serial,
                Ok(document(&b, "body", CACHE_BYTES * 3 / 4)),
                window,
                cx,
            );
            assert!(cache.ready(&a.path, a.token).is_none());
            assert!(cache.ready(&b.path, b.token).is_some());
            assert!(cache.test_stats().bytes <= CACHE_BYTES);
            cache.clear(cx);
        })
    });
}

#[gpui::test]
fn ready_entry_limit_keeps_visible_documents_and_evicts_only_after_lease_release(
    cx: &mut TestAppContext,
) {
    let (view, cx) = cx.add_window_view(|window, cx| Workspace::new(None, window, cx));
    let cache = view.update(cx, |workspace, cx| {
        let owner = cx.entity().downgrade();
        let preview = workspace.preview_pane.downgrade();
        cx.new(|cx| {
            let mut cache = Cache::new(owner, preview, cx);
            cache.test_loader(Arc::new(|_, _| Box::pin(std::future::pending())));
            cache
        })
    });
    cx.update(|window, cx| {
        cache.update(cx, |cache, cx| {
            let mut leases = Vec::new();
            for index in 0..MAX_ENTRIES {
                let input = input(&format!("{index}.md"), 1);
                leases.push(cache.request(input.clone(), window, cx).lease);
                let serial = cache.test_serial(&input).unwrap();
                cache.test_publish(&input, serial, Ok(document(&input, "body", 1)), window, cx);
            }
            assert_eq!(cache.test_stats().ready, MAX_ENTRIES);
            let next = input("next.md", 1);
            assert!(matches!(
                cache.request(next.clone(), window, cx).status,
                Status::Failed(_)
            ));
            assert_eq!(cache.test_stats().entries, MAX_ENTRIES);
            assert!(cache.ready(Path::new("0.md"), next.token).is_some());
            drop(leases.remove(0));
            let _next_lease = cache.request(next, window, cx).lease;
            assert!(
                cache
                    .ready(Path::new("0.md"), SourceToken::saved_fixture(1))
                    .is_none()
            );
            assert_eq!(cache.test_stats().entries, MAX_ENTRIES);
            cache.clear(cx);
        })
    });
}

#[gpui::test]
fn a_single_oversized_document_does_not_evict_other_ready_notes(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| Workspace::new(None, window, cx));
    let cache = view.update(cx, |workspace, cx| {
        let owner = cx.entity().downgrade();
        let preview = workspace.preview_pane.downgrade();
        cx.new(|cx| {
            let mut cache = Cache::new(owner, preview, cx);
            cache.test_loader(Arc::new(|_, _| Box::pin(std::future::pending())));
            cache
        })
    });
    cx.update(|window, cx| {
        cache.update(cx, |cache, cx| {
            let a = input("A.md", 1);
            let lease_a = cache.request(a.clone(), window, cx).lease;
            let serial = cache.test_serial(&a).unwrap();
            cache.test_publish(&a, serial, Ok(document(&a, "keep this", 1024)), window, cx);
            drop(lease_a);
            let b = input("TooLarge.md", 1);
            let _lease_b = cache.request(b.clone(), window, cx).lease;
            let serial = cache.test_serial(&b).unwrap();
            cache.test_publish(
                &b,
                serial,
                Ok(document(&b, "oversized preparation", CACHE_BYTES + 1)),
                window,
                cx,
            );
            assert_eq!(
                cache.ready(&a.path, a.token).unwrap().source.as_str(),
                "keep this"
            );
            let Status::Failed(message) = cache.request(b, window, cx).status else {
                panic!("oversized embed must fail")
            };
            assert!(message.contains("too large"));
            assert!(message.contains("Open"));
            assert!(!message.contains("scroll"));
            assert_eq!(cache.test_stats().bytes, 1024);
            assert_eq!(cache.test_stats().ready, 1);
            cache.clear(cx);
        })
    });
}

#[gpui::test]
fn oversized_open_snapshots_are_rejected_before_the_queue_can_retain_them(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| Workspace::new(None, window, cx));
    let source = Arc::new("x".repeat(8 * 1024 * 1024 + 1));
    let (cache, token) = view.update(cx, |workspace, cx| {
        let owner = cx.entity().downgrade();
        let preview = workspace.preview_pane.downgrade();
        let token = SourceToken::Open {
            editor: cx.entity().entity_id(),
            revision: 901,
        };
        (cx.new(|cx| Cache::new(owner, preview, cx)), token)
    });
    cx.update(|window, cx| {
        cache.update(cx, |cache, cx| {
            for index in 0..32 {
                let snapshot = cache.request(
                    Input {
                        path: format!("{index}.md").into(),
                        absolute_path: "unused".into(),
                        token,
                        open: Some((source.clone(), None)),
                    },
                    window,
                    cx,
                );
                let Status::Failed(message) = snapshot.status else {
                    panic!("oversized snapshot must fail")
                };
                assert!(message.contains("8 MiB"));
            }
            assert_eq!(cache.test_stats().entries, 0);
            assert_eq!(cache.test_stats().active, 0);
            assert_eq!(cache.test_stats().pending, 0);
            assert_eq!(cache.test_stats().bytes, 0);
        })
    });
    cx.run_until_parked();
    assert_eq!(Arc::strong_count(&source), 1);
}

#[gpui::test]
fn saved_embed_identity_ignores_unrelated_updates_but_tracks_its_own_content(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    for (name, text) in [("A.md", "Host"), ("B.md", "Target"), ("C.md", "Unrelated")] {
        std::fs::write(folder.path().join(name), text).unwrap();
    }
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
    cx.run_until_parked();
    let (cache, original, lease, ready) = view.update_in(cx, |workspace, window, cx| {
        let input = workspace.embed_input(Path::new("B.md"), cx).unwrap();
        assert!(matches!(
            input.token,
            SourceToken::Saved {
                fingerprint: Some(_),
                ..
            }
        ));
        let cache = workspace.embedded_documents.clone();
        let (lease, ready) = cache.update(cx, |cache, cx| {
            cache.test_loader(Arc::new(|_, _| Box::pin(std::future::pending())));
            let lease = cache.request(input.clone(), window, cx).lease;
            let serial = cache.test_serial(&input).unwrap();
            cache.test_publish(
                &input,
                serial,
                Ok(document(&input, "Target", 1024)),
                window,
                cx,
            );
            (lease, cache.ready(&input.path, input.token).unwrap())
        });
        (cache, input, lease, ready)
    });
    std::fs::write(folder.path().join("C.md"), "Unrelated changed").unwrap();
    view.update_in(cx, |workspace, window, cx| {
        workspace.vault.as_mut().unwrap().refresh().unwrap();
        workspace.revision += 1;
        let current = workspace.embed_input(Path::new("B.md"), cx).unwrap();
        assert_eq!(current.token, original.token);
        cache.update(cx, |cache, cx| {
            let snapshot = cache.request(current, window, cx);
            assert!(matches!(snapshot.status, Status::Ready));
            assert!(Arc::ptr_eq(snapshot.document.as_ref().unwrap(), &ready));
            assert_eq!(cache.test_stats().active, 0);
        });
    });
    std::fs::write(folder.path().join("B.md"), "Target changed").unwrap();
    view.update_in(cx, |workspace, window, cx| {
        workspace.vault.as_mut().unwrap().refresh().unwrap();
        workspace.revision += 1;
        let changed = workspace.embed_input(Path::new("B.md"), cx).unwrap();
        assert_ne!(changed.token, original.token);
        cache.update(cx, |cache, cx| {
            let snapshot = cache.request(changed, window, cx);
            assert!(matches!(snapshot.status, Status::Waiting));
            assert!(snapshot.document.is_none());
            assert!(cache.ready(&original.path, original.token).is_none());
            cache.clear(cx);
        });
    });
    drop(lease);
}
