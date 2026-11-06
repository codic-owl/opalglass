//! Explorer command menus and move-folder picker presentation.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExplorerAction {
    NewNote,
    NewFolder,
    Rename,
    Move,
    Trash,
    RestoreTrash,
    CopyPath,
    CopyRelativePath,
}

pub(super) struct ExplorerMenu {
    pub(super) target: Option<PathBuf>,
    pub(super) position: Point<Pixels>,
    pub(super) selected: usize,
}

impl Workspace {
    pub(super) fn open_explorer_menu(
        &mut self,
        target: Option<PathBuf>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.managing || self.vault.is_none() {
            return;
        }
        self.dismiss(window, cx);
        self.navigation.selected = target.clone();
        self.explorer_menu = Some(ExplorerMenu {
            target,
            position,
            selected: 0,
        });
        self.tree_focus.focus(window);
        cx.notify();
    }
    pub(super) fn explorer_actions(&self) -> Vec<(ExplorerAction, &'static str, &'static str)> {
        let Some(menu) = &self.explorer_menu else {
            return Vec::new();
        };
        let folder = menu
            .target
            .as_ref()
            .is_none_or(|p| self.vault.as_ref().is_some_and(|v| v.folders.contains(p)));
        let mut actions = Vec::new();
        if folder {
            actions.push((ExplorerAction::NewNote, "New note", "Ctrl N"));
            actions.push((ExplorerAction::NewFolder, "New folder", "Ctrl Shift N"));
        }
        if menu.target.is_some() {
            actions.push((ExplorerAction::Rename, "Rename…", "F2"));
            actions.push((
                ExplorerAction::Move,
                if folder {
                    "Move folder to…"
                } else {
                    "Move file to…"
                },
                "",
            ));
        }
        if menu.target.is_some() {
            actions.push((ExplorerAction::Trash, "Move to trash…", "Delete"));
        }
        actions.push((
            ExplorerAction::RestoreTrash,
            "Restore last deleted item",
            "",
        ));
        actions.push((ExplorerAction::CopyPath, "Copy path", ""));
        actions.push((ExplorerAction::CopyRelativePath, "Copy relative path", ""));
        actions
    }
    pub(super) fn run_explorer_action(
        &mut self,
        action: ExplorerAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.explorer_menu.take() else {
            return;
        };
        if matches!(
            action,
            ExplorerAction::CopyPath | ExplorerAction::CopyRelativePath
        ) {
            let Some(vault) = &self.vault else {
                return;
            };
            let target = menu.target.as_deref().unwrap_or(std::path::Path::new(""));
            // Vault roots and explorer targets are already known. Joining them
            // is lexical and never canonicalizes or reads the filesystem.
            let text = if action == ExplorerAction::CopyPath {
                if target.as_os_str().is_empty() {
                    vault.root.to_string_lossy().into_owned()
                } else {
                    vault.root.join(target).to_string_lossy().into_owned()
                }
            } else if target.as_os_str().is_empty() {
                ".".to_owned()
            } else {
                target.to_string_lossy().into_owned()
            };
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            cx.notify();
            return;
        }
        let action = match action {
            ExplorerAction::NewNote => Modal::New,
            ExplorerAction::NewFolder => Modal::NewFolder,
            ExplorerAction::Rename => Modal::Rename,
            ExplorerAction::Move => Modal::Move,
            ExplorerAction::Trash => Modal::Delete,
            ExplorerAction::RestoreTrash => Modal::RestoreTrash,
            ExplorerAction::CopyPath | ExplorerAction::CopyRelativePath => unreachable!(),
        };
        self.navigation.selected = menu.target;
        if action == Modal::RestoreTrash {
            self.change_trash(None, window, cx);
        } else if matches!(action, Modal::Move | Modal::Delete) {
            self.show_modal(action, window, cx);
        } else {
            self.show_inline(action, window, cx);
        }
    }
    pub(super) fn render_explorer_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let menu = self.explorer_menu.as_ref().unwrap();
        let mut content = div()
            .id("explorer-menu")
            .debug_selector(|| "explorer-menu".into())
            .w(px(250.))
            .p_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(LINE))
            .bg(rgb(PANEL))
            .shadow_lg()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.explorer_menu = None;
                cx.notify();
            }));
        for (index, (action, label, shortcut)) in self.explorer_actions().into_iter().enumerate() {
            content = content.child(
                div()
                    .id(("explorer-action", index))
                    .debug_selector(move || format!("explorer-action-{index}"))
                    .h(px(32.))
                    .px_3()
                    .rounded_sm()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_sm()
                    .cursor_pointer()
                    .when(index == menu.selected, |el| el.bg(rgb(0x343049)))
                    .hover(|s| s.bg(rgb(0x343049)))
                    .child(label)
                    .child(caption(shortcut))
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if let Some(menu) = &mut this.explorer_menu
                            && menu.selected != index
                        {
                            menu.selected = index;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run_explorer_action(action, window, cx);
                        cx.stop_propagation();
                    })),
            );
        }
        deferred(
            anchored()
                .position(menu.position)
                .snap_to_window_with_margin(px(8.))
                .child(content),
        )
        .with_priority(1)
        .into_any_element()
    }
    pub(super) fn render_move_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .absolute()
            .inset_0()
            .occlude()
            .bg(rgba(0x05070bb0))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(500.))
                    .p_5()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(PANEL))
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_lg().child("Move to folder"))
                    .child(
                        caption(
                            self.modal_target
                                .as_ref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default(),
                        )
                        .truncate(),
                    )
                    .child(
                        div()
                            .h(px(40.))
                            .border_1()
                            .border_color(rgb(ACCENT))
                            .rounded_md()
                            .bg(rgb(BG))
                            .child(self.input.clone()),
                    )
                    .child(caption("Search folders · ↑ ↓ to choose · Enter to move"))
                    .when(self.move_results.is_empty(), |el| {
                        el.child(caption("No matching destination folders"))
                    })
                    .child(
                        uniform_list(
                            "move-folders",
                            self.move_results.len(),
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .map(|index| {
                                        let path = this.move_results[index].clone();
                                        let label = if path.as_os_str().is_empty() {
                                            "/".into()
                                        } else {
                                            path.display().to_string()
                                        };
                                        div()
                                            .id(("move-folder", index))
                                            .debug_selector(move || format!("move-folder-{index}"))
                                            .h(px(34.))
                                            .px_3()
                                            .py_1()
                                            .rounded_md()
                                            .text_sm()
                                            .cursor_pointer()
                                            .when(index == this.move_selected, |el| {
                                                el.bg(rgb(0x343049))
                                            })
                                            .hover(|s| s.bg(rgb(0x303749)))
                                            .child(label)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.choose_move_folder(path.clone(), window, cx)
                                            }))
                                    })
                                    .collect()
                            }),
                        )
                        .h(px(240.))
                        .track_scroll(self.move_scroll.clone()),
                    )
                    .when(self.error, |el| {
                        el.child(
                            div()
                                .text_sm()
                                .text_color(rgb(0xf1b18e))
                                .child(self.status.clone()),
                        )
                    })
                    .child(
                        div().flex().justify_end().child(
                            button("cancel-move", "Cancel").on_click(
                                cx.listener(|this, _, window, cx| this.dismiss(window, cx)),
                            ),
                        ),
                    ),
            )
            .into_any_element()
    }
}
