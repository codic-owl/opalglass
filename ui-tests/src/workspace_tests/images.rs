use super::{TestAppContext, ViewMode, Workspace};

const PHOTO: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"100\"><rect width=\"200\" height=\"100\" fill=\"#73b4a2\"/></svg>";

#[gpui::test]
fn requested_image_sizes_preserve_ratio_and_fit_reading_and_split_panes(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let source =
        "![160](Photo.svg)\n\n![[Photo.svg|120x60]]\n\n![**caption**|800x400](Photo.svg)\n";
    std::fs::write(folder.path().join("A.md"), source).unwrap();
    std::fs::write(folder.path().join("Photo.svg"), PHOTO).unwrap();
    let (view, cx) = cx.add_window_view(|w, cx| Workspace::new(Some(folder.path().into()), w, cx));
    view.update_in(cx, |this, w, cx| this.set_mode(ViewMode::Preview, w, cx));
    cx.simulate_resize(gpui::size(gpui::px(1100.), gpui::px(800.)));
    cx.run_until_parked();
    let width = cx.debug_bounds("attachment-image-0-0").unwrap();
    assert!((f32::from(width.size.width) - 160.).abs() < 1.);
    assert!((f32::from(width.size.height) - 80.).abs() < 1.);
    let explicit = cx.debug_bounds("attachment-image-1-0").unwrap();
    assert!((f32::from(explicit.size.width) - 120.).abs() < 1.);
    assert!((f32::from(explicit.size.height) - 60.).abs() < 1.);
    let large = cx.debug_bounds("attachment-image-2-0").unwrap();
    let column = cx.debug_bounds("preview-column-2").unwrap();
    assert!(large.right() <= column.right() + gpui::px(1.));
    assert!((f32::from(large.size.width) / f32::from(large.size.height) - 2.).abs() < 0.01);
    assert!(
        cx.debug_bounds("attachment-image-2-1").is_none(),
        "formatted alt text must render one image"
    );
    view.update_in(cx, |this, w, cx| this.set_mode(ViewMode::Split, w, cx));
    cx.simulate_resize(gpui::size(gpui::px(800.), gpui::px(800.)));
    cx.run_until_parked();
    let large = cx.debug_bounds("attachment-image-2-0").unwrap();
    let column = cx.debug_bounds("preview-column-2").unwrap();
    assert!(large.right() <= column.right() + gpui::px(1.));
    assert!(f32::from(large.size.width) < 400.);
    assert!((f32::from(large.size.width) / f32::from(large.size.height) - 2.).abs() < 0.01);
    view.read_with(cx, |this, cx| {
        assert_eq!(this.tabs[this.active].editor.read(cx).buffer.text(), source);
    });
}
