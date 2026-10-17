use super::*;
use crate::workspace::embedded_documents::{Input, SourceToken};
use opalglass::prepared::Prepared;

#[gpui::test]
fn visible_nested_content_keeps_its_source_and_ancestors_after_idle_and_cache_pressure(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    let host = "# Host\n\n![[Parent]]\n";
    let parent = format!(
        "# Parent\n\n{}![[Child]]\n",
        "Parent paragraph.\n\n".repeat(150)
    );
    let child = format!(
        "```txt\nVISIBLE CHILD\n```\n\n{}",
        "Child paragraph.\n\n".repeat(100)
    );
    std::fs::write(folder.path().join("Host.md"), host).unwrap();
    std::fs::write(folder.path().join("Parent.md"), parent).unwrap();
    std::fs::write(folder.path().join("Child.md"), child).unwrap();
    let (view, cx) = reading(cx, folder.path());
    let parent = instances(&view, cx, "Parent")[0].id;
    show_instance(&view, cx, parent);
    let child = instances(&view, cx, "Child")[0].id;
    show_instance(&view, cx, child);
    copy_code(&view, cx, child, "VISIBLE CHILD\n");
    // The parent's body is now above the viewport. Its document is needed by
    // the visible descendant even while the app is idle between paints.
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(600));
    settle(cx);
    let inputs = view.read_with(cx, |this, _| {
        [parent, child].map(|id| {
            this.tabs[this.active].composition.instances[&id]
                .input
                .clone()
                .unwrap()
        })
    });
    let pressure_source = Arc::new("Cache pressure fixture.\n".to_owned());
    let pressure_prepared = Arc::new(Prepared::new(&pressure_source));
    view.update_in(cx, |this, window, cx| {
        this.embedded_documents.update(cx, |cache, cx| {
            cache.test_loader(Arc::new(|_, _| Box::pin(std::future::pending())));
            for index in 0..70 {
                let input = Input {
                    path: format!("pressure-{index}.md").into(),
                    absolute_path: format!("/unused/pressure-{index}.md").into(),
                    token: SourceToken::Saved {
                        vault_generation: u64::MAX,
                        fingerprint: None,
                    },
                    open: None,
                };
                let snapshot = cache.request(input.clone(), window, cx);
                let serial = cache.test_serial(&input).unwrap();
                cache.test_publish(
                    &input,
                    serial,
                    Ok(Arc::new(Document {
                        path: input.path.clone(),
                        source: pressure_source.clone(),
                        prepared: pressure_prepared.clone(),
                        token: input.token,
                        projection: opalglass::callouts::Projection::default(),
                        bytes: 1024 * 1024,
                    })),
                    window,
                    cx,
                );
                drop(snapshot);
            }
            assert!(cache.test_stats().entries <= 64);
            assert!(cache.test_stats().bytes <= 64 * 1024 * 1024);
            for input in &inputs {
                assert!(
                    cache.ready(&input.path, input.token).is_some(),
                    "visible source {} must remain pinned",
                    input.path.display()
                );
            }
        });
    });
    settle(cx);
    for id in [parent, child] {
        let _ = document(&view, cx, id);
    }
    copy_code(&view, cx, child, "VISIBLE CHILD\n");
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[this.active].editor.read(cx).buffer.text(), host)
    });
}
