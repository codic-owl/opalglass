use super::{ViewMode, Workspace};
use gpui::{Entity, ListOffset, RenderImage, TestAppContext, VisualTestContext, px, size};
use std::sync::Arc;

const URL: &str = "https://images.invalid/photo.qoi";

fn wide_image() -> Arc<RenderImage> {
    // A 2×1 black QOI image: one two-pixel run and the standard end marker.
    opalglass::images::prepare(b"qoif\0\0\0\x02\0\0\0\x01\x04\0\xc1\0\0\0\0\0\0\0\x01").unwrap()
}

fn loading_preview(view: &Entity<Workspace>, cx: &mut VisualTestContext) {
    view.update_in(cx, |this, window, cx| {
        this.remote_images.update(cx, |cache, _| {
            cache.test_loader(Arc::new(|_| Box::pin(std::future::pending())));
        });
        this.set_mode(ViewMode::Preview, window, cx);
    });
    cx.simulate_resize(size(px(1100.), px(1200.)));
    cx.run_until_parked();
}

fn complete(view: &Entity<Workspace>, cx: &mut VisualTestContext) {
    view.update_in(cx, |this, window, cx| {
        this.remote_images.update(cx, |cache, cx| {
            let generation = cache.test_generation(URL);
            cache.test_publish(URL, generation, Ok(wide_image()), window, cx);
        });
    });
    cx.run_until_parked();
}

#[gpui::test]
fn width_only_remote_images_adopt_ratio_without_resizing_explicit_or_default_frames(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    let source = format!("![160]({URL})\n\n![120x60]({URL})\n\n![ordinary]({URL})\n");
    std::fs::write(folder.path().join("A.md"), source).unwrap();
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
    loading_preview(&view, cx);
    let loading = cx.debug_bounds("remote-image-0-0").unwrap();
    assert!((f32::from(loading.size.width) - 160.).abs() < 1.);
    assert!((f32::from(loading.size.height) - 160.).abs() < 1.);
    let explicit = cx.debug_bounds("remote-image-1-0").unwrap();
    let default = cx.debug_bounds("remote-image-2-0").unwrap();
    assert!((f32::from(default.size.height) - 320.).abs() < 1.);
    complete(&view, cx);
    let loaded = cx.debug_bounds("remote-image-0-0").unwrap();
    assert!((f32::from(loaded.size.width) - 160.).abs() < 1.);
    assert!((f32::from(loaded.size.height) - 80.).abs() < 1.);
    assert_eq!(
        cx.debug_bounds("remote-image-1-0").unwrap().size,
        explicit.size
    );
    assert_eq!(
        cx.debug_bounds("remote-image-2-0").unwrap().size,
        default.size
    );
}

#[gpui::test]
fn remote_image_finishing_above_reading_keeps_the_logical_anchor(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let source = format!(
        "![160]({URL})\n\n{}",
        "Following paragraph.\n\n".repeat(100)
    );
    std::fs::write(folder.path().join("A.md"), source).unwrap();
    let (view, cx) =
        cx.add_window_view(|window, cx| Workspace::new(Some(folder.path().into()), window, cx));
    loading_preview(&view, cx);
    view.update(cx, |this, cx| {
        this.tabs[this.active].preview_scroll.scroll_to(ListOffset {
            item_ix: 12,
            offset_in_item: px(7.),
        });
        this.preview_pane.update(cx, |_, cx| cx.notify());
    });
    cx.run_until_parked();
    let before = view.read_with(cx, |this, _| {
        this.tabs[this.active].preview_scroll.logical_scroll_top()
    });
    let position = cx.debug_bounds("preview-column-12").unwrap().origin;
    complete(&view, cx);
    let after = view.read_with(cx, |this, _| {
        this.tabs[this.active].preview_scroll.logical_scroll_top()
    });
    assert_eq!(before.item_ix, after.item_ix);
    assert_eq!(before.offset_in_item, after.offset_in_item);
    assert_eq!(
        position,
        cx.debug_bounds("preview-column-12").unwrap().origin
    );
    view.update(cx, |this, cx| {
        this.tabs[this.active].preview_scroll.scroll_to(ListOffset {
            item_ix: 0,
            offset_in_item: px(0.),
        });
        this.preview_pane.update(cx, |_, cx| cx.notify());
    });
    cx.run_until_parked();
    let loaded = cx.debug_bounds("remote-image-0-0").unwrap();
    assert!((f32::from(loaded.size.height) - 80.).abs() < 1.);
}

#[gpui::test]
fn requested_remote_sizes_fit_narrow_reading_and_split_panes(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let source = format!("![800]({URL})\n\n![800x400]({URL})\n");
    std::fs::write(folder.path().join("A.md"), source).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    loading_preview(&view, cx);
    complete(&view, cx);
    for (mode, width) in [(ViewMode::Preview, 1100.), (ViewMode::Split, 880.)] {
        view.update_in(cx, |this, w, cx| this.set_mode(mode, w, cx));
        cx.simulate_resize(size(px(width), px(1200.)));
        cx.update(|w, _| w.refresh());
        cx.run_until_parked();
        for (image, column) in [
            ("remote-image-0-0", "preview-column-0"),
            ("remote-image-1-0", "preview-column-1"),
        ] {
            let image = cx.debug_bounds(image).unwrap();
            let column = cx.debug_bounds(column).unwrap();
            assert!(image.size.width <= column.size.width);
            assert!(image.left() >= column.left() - px(1.));
            assert!(image.right() <= column.right() + px(1.));
            assert!((f32::from(image.size.width) / f32::from(image.size.height) - 2.).abs() < 0.01);
        }
    }
}
