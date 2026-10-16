mod editor;
mod theme;
mod workspace;

use gpui::{prelude::*, *};
use std::path::PathBuf;
use workspace::Workspace;

// Return temporary indexing allocations without relying on libc arena retention.
#[global_allocator]
static ALLOCATOR: opalglass::Allocator = opalglass::Allocator;

fn main() {
    let arg = std::env::args_os().nth(1);
    if arg.as_deref().is_some_and(|s| s == "--help" || s == "-h") {
        println!(
            "Opalglass — native Markdown workspace\n\nUsage: opalglass [VAULT_DIRECTORY]\n\nWith no directory, reopen the last vault or use Open vault in the app."
        );
        return;
    }
    let path = arg.map(PathBuf::from);
    Application::new().run(move |cx: &mut App| {
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1360.), px(860.)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Opalglass".into()),
                    ..Default::default()
                }),
                window_min_size: Some(size(px(880.), px(560.))),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| Workspace::new(path, window, cx));
                let weak = view.downgrade();
                window.on_window_should_close(cx, move |window, cx| {
                    weak.update(cx, |this, cx| this.save_all_in(window, cx))
                        .unwrap_or(true)
                });
                view
            },
        )
        .expect("Unable to create the Opalglass window");
        cx.activate(true);
    });
}
