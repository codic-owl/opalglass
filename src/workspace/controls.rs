// Shared buttons, vector icons and labels.
use super::*;

pub(super) fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .text_sm()
        .cursor_pointer()
        .text_color(rgb(TEXT))
        .hover(|s| s.bg(rgb(0x303749)))
        .child(label.into())
}
pub(super) fn caption(text: impl Into<SharedString>) -> Div {
    div().text_xs().text_color(rgb(MUTED)).child(text.into())
}

#[derive(Clone, Copy)]
pub(super) enum ToolbarIcon {
    Commands,
    Bookmark,
    Calendar,
    Back,
    Forward,
    Settings,
    Outline,
    Locked,
    Unlocked,
    Source,
    Split,
    Reading,
    Save,
    Copy,
    Reload,
    Panel,
    NewNote,
    NewFolder,
    Sort,
    AutoReveal,
    Collapse,
    Expand,
}

pub(super) fn icon_button(
    id: &'static str,
    icon: ToolbarIcon,
    label: String,
    enabled: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .size(px(28.))
        .flex_shrink_0()
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .when(enabled, |el| {
            el.cursor_pointer().hover(|s| s.bg(rgb(0x303749)))
        })
        .when(!enabled, |el| el.opacity(0.35))
        .tooltip(move |_, cx| cx.new(|_| PathTooltip(label.clone())).into())
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    // Small native vector icons, using a consistent 24-unit grid.
                    let lines: &[&[(f32, f32)]] = match icon {
                        ToolbarIcon::Commands => &[
                            &[(4., 6.), (10., 12.), (4., 18.)],
                            &[(13., 18.), (21., 18.)],
                        ],
                        ToolbarIcon::Bookmark => &[&[
                            (6., 3.),
                            (18., 3.),
                            (18., 21.),
                            (12., 16.),
                            (6., 21.),
                            (6., 3.),
                        ]],
                        ToolbarIcon::Calendar => &[
                            &[(3., 5.), (21., 5.), (21., 21.), (3., 21.), (3., 5.)],
                            &[(3., 10.), (21., 10.)],
                            &[(8., 2.), (8., 7.)],
                            &[(16., 2.), (16., 7.)],
                        ],
                        ToolbarIcon::Back => &[
                            &[(14., 5.), (7., 12.), (14., 19.)],
                            &[(7., 12.), (21., 12.)],
                        ],
                        ToolbarIcon::Forward => &[
                            &[(10., 5.), (17., 12.), (10., 19.)],
                            &[(3., 12.), (17., 12.)],
                        ],
                        ToolbarIcon::Settings => &[
                            &[(4., 6.), (20., 6.)],
                            &[(4., 12.), (20., 12.)],
                            &[(4., 18.), (20., 18.)],
                            &[(8., 3.), (8., 9.)],
                            &[(16., 9.), (16., 15.)],
                            &[(10., 15.), (10., 21.)],
                        ],
                        ToolbarIcon::Outline => &[
                            &[(3., 5.), (21., 5.)],
                            &[(8., 10.), (21., 10.)],
                            &[(8., 15.), (21., 15.)],
                            &[(3., 20.), (21., 20.)],
                        ],
                        ToolbarIcon::Locked => &[
                            &[(5., 10.), (19., 10.), (19., 21.), (5., 21.), (5., 10.)],
                            &[
                                (8., 10.),
                                (8., 6.),
                                (9., 3.),
                                (15., 3.),
                                (16., 6.),
                                (16., 10.),
                            ],
                            &[(12., 14.), (12., 17.)],
                        ],
                        ToolbarIcon::Unlocked => &[
                            &[(5., 10.), (19., 10.), (19., 21.), (5., 21.), (5., 10.)],
                            &[(8., 10.), (8., 6.), (9., 3.), (15., 3.), (16., 6.)],
                            &[(12., 14.), (12., 17.)],
                        ],
                        ToolbarIcon::Source => &[
                            &[(8., 6.), (2., 12.), (8., 18.)],
                            &[(16., 6.), (22., 12.), (16., 18.)],
                            &[(14., 3.), (10., 21.)],
                        ],
                        ToolbarIcon::Split => &[
                            &[(3., 4.), (21., 4.), (21., 20.), (3., 20.), (3., 4.)],
                            &[(12., 4.), (12., 20.)],
                        ],
                        ToolbarIcon::Reading => &[&[
                            (12., 6.),
                            (9., 4.),
                            (2., 4.),
                            (2., 19.),
                            (9., 19.),
                            (12., 21.),
                            (15., 19.),
                            (22., 19.),
                            (22., 4.),
                            (15., 4.),
                            (12., 6.),
                            (12., 21.),
                        ]],
                        ToolbarIcon::Save => &[
                            &[
                                (3., 3.),
                                (17., 3.),
                                (21., 7.),
                                (21., 21.),
                                (3., 21.),
                                (3., 3.),
                            ],
                            &[(7., 3.), (7., 9.), (16., 9.), (16., 3.)],
                            &[(7., 21.), (7., 14.), (17., 14.), (17., 21.)],
                        ],
                        ToolbarIcon::Copy => &[
                            &[(8., 8.), (21., 8.), (21., 21.), (8., 21.), (8., 8.)],
                            &[(16., 5.), (16., 2.), (2., 2.), (2., 16.), (5., 16.)],
                        ],
                        ToolbarIcon::Reload => &[
                            &[
                                (20., 10.),
                                (18., 6.),
                                (14., 3.),
                                (9., 3.),
                                (5., 5.),
                                (3., 9.),
                                (3., 14.),
                                (6., 19.),
                                (11., 21.),
                                (16., 20.),
                                (20., 16.),
                            ],
                            &[(20., 3.), (20., 10.), (13., 10.)],
                        ],
                        ToolbarIcon::Panel => &[
                            &[(4., 3.), (20., 3.), (20., 21.), (4., 21.), (4., 3.)],
                            &[(9., 3.), (9., 21.)],
                        ],
                        ToolbarIcon::NewNote => &[
                            &[(10., 5.), (4., 5.), (4., 21.), (20., 21.), (20., 14.)],
                            &[
                                (10., 14.),
                                (11., 10.),
                                (19., 2.),
                                (22., 5.),
                                (14., 13.),
                                (10., 14.),
                            ],
                            &[(17., 4.), (20., 7.)],
                        ],
                        ToolbarIcon::NewFolder => &[
                            &[
                                (3., 6.),
                                (9., 6.),
                                (11., 9.),
                                (21., 9.),
                                (21., 20.),
                                (3., 20.),
                                (3., 6.),
                            ],
                            &[(12., 11.), (12., 17.)],
                            &[(9., 14.), (15., 14.)],
                        ],
                        ToolbarIcon::Sort => &[
                            &[(5., 20.), (5., 4.)],
                            &[(2., 7.), (5., 4.), (8., 7.)],
                            &[(11., 6.), (15., 6.)],
                            &[(11., 12.), (18., 12.)],
                            &[(11., 18.), (22., 18.)],
                        ],
                        ToolbarIcon::AutoReveal => &[
                            &[(4., 3.), (20., 3.)],
                            &[(4., 21.), (20., 21.)],
                            &[(4., 7.), (20., 7.), (20., 17.), (4., 17.), (4., 7.)],
                        ],
                        ToolbarIcon::Collapse => &[
                            &[(7., 3.), (12., 8.), (17., 3.)],
                            &[(7., 21.), (12., 16.), (17., 21.)],
                        ],
                        ToolbarIcon::Expand => &[
                            &[(7., 8.), (12., 3.), (17., 8.)],
                            &[(7., 16.), (12., 21.), (17., 16.)],
                        ],
                    };
                    for line in lines {
                        let mut path = PathBuilder::stroke(px(1.5));
                        for (index, &(x, y)) in line.iter().enumerate() {
                            let p = bounds.origin
                                + point(
                                    bounds.size.width * (x / 24.),
                                    bounds.size.height * (y / 24.),
                                );
                            if index == 0 {
                                path.move_to(p);
                            } else {
                                path.line_to(p);
                            }
                        }
                        if let Ok(path) = path.build() {
                            window.paint_path(path, rgb(MUTED));
                        }
                    }
                },
            )
            .size(px(18.)),
        )
}
