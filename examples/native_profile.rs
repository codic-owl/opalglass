//! Real GPUI window, native font shaping, compositor and GPU. Uses only temporary notes.
#[path = "../src/editor.rs"]
mod editor;
#[path = "../src/theme.rs"]
mod theme;
mod workspace {
    include!("../src/workspace.rs");

    impl Workspace {
        pub fn profile(
            &self,
            fixture: tempfile::TempDir,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            cx.spawn_in(window, async move |this, cx| {
                let _fixture = fixture;
                Timer::after(Duration::from_secs(2)).await;
                for action in 0..12 {
                    let _ = this.update_in(cx, |this, window, cx| match action {
                        0 | 2 | 4 => this.open_note("B.md".into(), window, cx),
                        1 | 3 | 7 => this.close_tab(window, cx),
                        5 => this.open_note("C.md".into(), window, cx),
                        6 | 8 | 10 => this.activate_tab(0, window, cx),
                        9 | 11 => this.activate_tab(1, window, cx),
                        _ => unreachable!(),
                    });
                    Timer::after(Duration::from_millis(500)).await;
                }
                let _ = this.update_in(cx, |this, window, cx| {
                    println!("Native viewport: {:?}", window.viewport_size());
                    println!("{}", this.performance.read(cx).log_text());
                    if this.performance.read(cx).history.len() != 14 {
                        eprintln!("Incomplete or interrupted benchmark: expected 14 painted actions. Keep the window visible and do not interact while it runs.");
                    }
                    cx.quit();
                });
            })
            .detach();
        }
    }
}
use gpui::{prelude::*, *};

fn main() {
    let fixture = tempfile::tempdir().unwrap();
    std::fs::write(
        fixture.path().join("A.md"),
        "# A\n\nShort note. [[B]] [[C]]\n",
    )
    .unwrap();
    std::fs::write(
        fixture.path().join("B.md"),
        "# B\n\nAnother short note. [[A]]\n",
    )
    .unwrap();
    let text = std::env::var_os("OPALGLASS_PROFILE_NOTE")
        .map(|p| std::fs::read_to_string(p).unwrap())
        .unwrap_or_else(|| {
            // Distinct paragraphs exercise real shaping; repeating a single
            // paragraph hides the cost behind text-layout cache hits.
            let words = [
                "interface",
                "computer",
                "configuration",
                "renderer",
                "paragraph",
                "testing",
                "document",
                "notes",
                "system",
                "design",
                "latency",
                "keyboard",
                "window",
                "performance",
                "source",
                "preview",
                "process",
                "memory",
                "frame",
            ];
            let mut seed = 12_u64;
            (0..1000)
                .map(|i| {
                    let paragraph = (0..35)
                        .map(|_| {
                            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                            words[(seed >> 32) as usize % words.len()]
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    format!("{paragraph} {i}.\n\n")
                })
                .collect::<String>()
        });
    std::fs::write(fixture.path().join("C.md"), text).unwrap();
    let incoming = std::env::var("OPALGLASS_PROFILE_BACKLINKS")
        .ok()
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(0);
    for i in 0..incoming {
        std::fs::write(
            fixture.path().join(format!("Incoming{i:04}.md")),
            "[[A]] [[B]] [[C]]",
        )
        .unwrap();
    }
    Application::new().run(move |cx: &mut App| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1360.), px(860.)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Opalglass native benchmark".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    let workspace =
                        workspace::Workspace::new(Some(fixture.path().into()), window, cx);
                    workspace.profile(fixture, window, cx);
                    workspace
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
