// File-action dialogs and submission.
use super::*;

impl Workspace {
    pub(super) fn show_modal(&mut self, modal: Modal, window: &mut Window, cx: &mut Context<Self>) {
        if self.managing {
            return;
        }
        self.note_generation += 1;
        self._note_load = None;
        self.note_loading = false;
        if matches!(
            modal,
            Modal::New
                | Modal::Copy
                | Modal::NewFolder
                | Modal::Rename
                | Modal::Move
                | Modal::Delete
        ) && self.vault.is_none()
        {
            self.message("Open a vault first", true, cx);
            return;
        }
        if matches!(modal, Modal::Copy | Modal::Reload) && self.tabs.is_empty() {
            return;
        }
        self.explorer_menu = None;
        self.sort_menu = None;
        self.inline_parent = None;
        self.error = false;
        self.modal_target = self.navigation.selected.clone();
        if matches!(modal, Modal::Rename | Modal::Move | Modal::Delete)
            && self.modal_target.is_none()
        {
            self.message("Select a file or folder first", false, cx);
            return;
        }
        let initial = if matches!(modal, Modal::New | Modal::NewFolder) {
            let folder = self.selected_folder();
            if folder.as_os_str().is_empty() {
                String::new()
            } else {
                match Vault::path_name(&folder) {
                    Ok(name) => format!("{name}/"),
                    Err(error) => {
                        self.message(format!("Cannot create here: {error}"), true, cx);
                        return;
                    }
                }
            }
        } else if modal == Modal::Rename {
            self.modal_target
                .as_ref()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        } else if modal == Modal::Move {
            let from = self.modal_target.as_ref().unwrap();
            let mut folders = vec![PathBuf::new()];
            folders.extend(self.vault.as_ref().unwrap().folders.iter().cloned());
            folders.retain(|p| from.parent() != Some(p.as_path()) && !p.starts_with(from));
            folders.sort_by_cached_key(|p| p.to_string_lossy().to_lowercase());
            self.move_candidates = folders.clone();
            self.move_results = folders;
            self.move_selected = 0;
            self.move_query.clear();
            self.move_scroll = UniformListScrollHandle::new();
            String::new()
        } else if modal == Modal::Copy {
            let name = match Vault::path_name(&self.tabs[self.active].path) {
                Ok(name) => name,
                Err(error) => {
                    self.message(format!("Cannot copy this note: {error}"), true, cx);
                    return;
                }
            };
            let stem = name
                .rsplit_once('.')
                .map_or(name.as_str(), |(stem, _)| stem);
            format!("{stem} copy")
        } else {
            String::new()
        };
        self.input.update(cx, |editor, cx| {
            editor.set_text(&initial, cx);
            if matches!(modal, Modal::New | Modal::NewFolder) {
                editor.buffer.move_to(initial.len(), false);
            } else {
                editor.buffer.select(0, initial.len());
            }
        });
        self.modal = modal;
        if modal == Modal::Delete {
            self.focus.focus(window);
        } else {
            self.input.read(cx).focus.focus(window);
        }
        cx.notify();
    }
    pub(super) fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.managing {
            return;
        }
        let inline = self.inline_parent.is_some();
        self.modal = Modal::None;
        self.inline_parent = None;
        self.explorer_menu = None;
        self.sort_menu = None;
        if inline {
            self.tree_focus.focus(window);
        } else {
            self.focus_document(window, cx);
        }
        cx.notify();
    }
    pub(super) fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.managing {
            return;
        }
        let mut input = self.input.read(cx).buffer.text().trim().to_owned();
        if self.inline_parent.is_some() && matches!(self.modal, Modal::New | Modal::NewFolder) {
            if input.is_empty() {
                self.message("Enter a name", true, cx);
                return;
            }
            let name = match Vault::folder_path(&input) {
                Ok(name) => name,
                Err(error) => {
                    self.message(error.to_string(), true, cx);
                    return;
                }
            };
            let path = self.inline_parent.as_ref().unwrap().join(name);
            input = match Vault::path_name(&path) {
                Ok(name) => name,
                Err(error) => {
                    self.message(error.to_string(), true, cx);
                    return;
                }
            };
        }
        match self.modal {
            Modal::OpenPath => self.open_vault(PathBuf::from(input), window, cx),
            Modal::New | Modal::Copy => {
                let text = if self.modal == Modal::Copy {
                    let Some(tab) = self.tabs.get(self.active) else {
                        self.dismiss(window, cx);
                        return;
                    };
                    tab.disk_text(cx)
                } else {
                    String::new()
                };
                if let Some(vault) = &mut self.vault {
                    match vault.create(&input, &text) {
                        Ok(path) => {
                            self.revision += 1;
                            self.reveal_path(&path);
                            self.message("Note created", false, cx);
                            self.open_note(path, window, cx);
                        }
                        Err(error) => {
                            self.message(format!("Could not create note: {error}"), true, cx)
                        }
                    }
                }
            }
            Modal::NewFolder => {
                if let Some(vault) = &mut self.vault {
                    match vault.create_folder(&input) {
                        Ok(path) => {
                            self.revision += 1;
                            self.navigation.expanded.insert(path.clone());
                            self.reveal_path(&path);
                            self.dismiss(window, cx);
                            self.tree_focus.focus(window);
                            self.message("Folder created", false, cx);
                        }
                        Err(error) => {
                            self.message(format!("Could not create folder: {error}"), true, cx)
                        }
                    }
                }
            }
            Modal::Move => {
                self.update_move_results(cx);
                if let Some(folder) = self.move_results.get(self.move_selected).cloned() {
                    self.choose_move_folder(folder, window, cx);
                }
            }
            Modal::Rename => {
                let Some(from) = self.modal_target.clone() else {
                    return;
                };
                let is_folder = self
                    .vault
                    .as_ref()
                    .is_some_and(|v| v.folders.contains(&from));
                let is_attachment = self.vault.as_ref().is_some_and(|v| v.files.contains(&from));
                let path = if is_folder || is_attachment {
                    Vault::folder_path(&input)
                } else {
                    Vault::note_path(&input)
                };
                let to = match path {
                    Ok(path) => path,
                    Err(error) => {
                        self.message(error.to_string(), true, cx);
                        return;
                    }
                };
                if to.components().count() != 1 {
                    self.message("Enter a single name; use Move to change folders", true, cx);
                    return;
                }
                let to = from.parent().unwrap_or(std::path::Path::new("")).join(to);
                self.move_entry(from, to, window, cx);
            }
            Modal::Reload => {
                self.reload_tab(self.active, cx);
                self.dismiss(window, cx);
            }
            Modal::Delete => {
                if let Some(path) = self.modal_target.clone() {
                    self.change_trash(Some(path), window, cx);
                }
            }
            Modal::RestoreTrash => self.change_trash(None, window, cx),
            Modal::None => (),
        }
    }
}
