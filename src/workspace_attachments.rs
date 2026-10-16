impl Workspace {
    fn import_attachments(
        &mut self,
        editor: Entity<Editor>,
        inputs: Vec<opalglass::attachments::Input>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.managing || self.checkpointing || inputs.is_empty() {
            return;
        }
        let Some(vault) = self.vault.as_ref() else {
            return;
        };
        let Some(tab) = self.tabs.iter().find(|tab| tab.editor == editor) else {
            return;
        };
        let root = vault.root.clone();
        let path = tab.path.clone();
        let revision = editor.read(cx).buffer.revision();
        let mut selection = editor.read(cx).buffer.selection();
        if let Some(header) = opalglass::properties::frontmatter(editor.read(cx).buffer.text())
            .ok()
            .flatten()
            && selection.start < header.range.end
        {
            selection.start = header.range.end;
            selection.end = selection.end.max(header.range.end);
        }
        self.managing = true;
        self.focus.focus(window);
        self.message("Importing attachments…", false, cx);
        self._management = Some(cx.spawn_in(window, async move |this, cx| {
            let destination = root.clone();
            let (imported, errors) = cx.background_executor().spawn(async move {
                let mut imported = vec![];
                let mut errors = vec![];
                for input in inputs {
                    match opalglass::attachments::import(&destination, input) {
                        Ok(path) => imported.push(path),
                        Err(error) => errors.push(error.to_string()),
                    }
                }
                (imported, errors)
            }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.managing = false;
                if this.vault.as_ref().is_none_or(|vault| vault.root != root)
                    || !this.tabs.iter().any(|tab| tab.editor == editor && tab.path == path)
                    || editor.read(cx).buffer.revision() != revision {
                    this.message("The note changed during import. Imported files are in Attachments; no note text was replaced.", true, cx);
                    return;
                }
                if !imported.is_empty() {
                    let links = imported.iter().map(|attachment| opalglass::attachments::link(&path, attachment)).collect::<Vec<_>>().join("\n\n");
                    editor.update(cx, |editor, cx| {
                        let before = if selection.start > 0 && !editor.buffer.text()[..selection.start].ends_with('\n') { "\n\n" } else { "" };
                        editor.buffer.replace(selection, &format!("{before}{links}\n\n"));
                        editor.reveal_cursor(cx);
                        editor.focus.focus(window);
                    });
                    this.revision += 1;
                }
                if errors.is_empty() { this.message(format!("Imported {} attachment(s)", imported.len()), false, cx); }
                else { this.message(format!("Imported {} attachment(s). {}", imported.len(), errors.join("; ")), true, cx); }
                cx.notify();
            });
        }));
    }
}
