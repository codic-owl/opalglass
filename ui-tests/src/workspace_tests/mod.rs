use super::{
    Modal, PathBuf, PickerMode, SIDEBAR_RAIL_WIDTH, SPLIT_DIVIDER_WIDTH, VaultIndexProgress,
    ViewMode, Workspace,
};
use gpui::TestAppContext;

mod explorer;
mod navigation;
mod performance;
mod rendering;
mod saving;
mod search;
mod virtualization;
mod workflows;

fn right_click(cx: &mut gpui::VisualTestContext, selector: &'static str) {
    let point = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_mouse_down(point, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.simulate_mouse_up(point, gpui::MouseButton::Right, gpui::Modifiers::default());
}

fn right_click_explorer_space(cx: &mut gpui::VisualTestContext) {
    let bounds = cx.debug_bounds("explorer").unwrap();
    let point = gpui::point(
        bounds.left() + gpui::px(12.),
        bounds.bottom() - gpui::px(8.),
    );
    cx.simulate_mouse_down(point, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.simulate_mouse_up(point, gpui::MouseButton::Right, gpui::Modifiers::default());
}

fn click(cx: &mut gpui::VisualTestContext, selector: &'static str) {
    let point = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_click(point, gpui::Modifiers::default());
}

fn drag(cx: &mut gpui::VisualTestContext, source: &'static str, target: &'static str) {
    let from = cx.debug_bounds(source).unwrap().center();
    let to = cx.debug_bounds(target).unwrap().center();
    cx.simulate_mouse_down(from, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_move(
        from + gpui::point(gpui::px(8.), gpui::px(0.)),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.simulate_mouse_move(to, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_up(to, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
}
