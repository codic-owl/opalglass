// Reference-preserving moves and rollback.
use super::*;

impl Vault {
    /// Move an entry and preserve resolved wiki, Markdown and attachment links. Callers
    /// save their open buffers first. All work may run on a background thread.
    pub fn move_entry(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        let source = self.checked_entry(from)?;
        let destination = self.checked_entry(to)?;
        if to == from || to.starts_with(from) {
            return Err(invalid("Choose a different location outside this folder"));
        }
        self.check_destination(to, Some(from))?;
        let is_folder = fs::metadata(&source)?.is_dir();
        if !is_folder
            && from
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md"))
                != to.extension().is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            return Err(invalid(
                "Notes must keep their .md extension; attachments cannot become notes",
            ));
        }
        self.refresh()?;
        // Build the post-move resolver before touching any files. This also
        // detects outbound links whose local or basename lookup would change.
        let mut after = self.clone();
        after.notes = Arc::new(
            self.notes
                .iter()
                .map(|note| {
                    let path = moved_path(&note.path, from, to);
                    if path == note.path {
                        return note.clone();
                    }
                    let mut new = (**note).clone();
                    new.path = path;
                    new.title = new
                        .path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into();
                    new.search_path = new.path.to_string_lossy().to_lowercase();
                    new.stamp = None;
                    Arc::new(new)
                })
                .collect(),
        );
        after.folders = Arc::new(
            self.folders
                .iter()
                .map(|p| moved_path(p, from, to))
                .collect(),
        );
        after.files = Arc::new(self.files.iter().map(|p| moved_path(p, from, to)).collect());
        after.reindex();
        let before_paths = &self.indexes.references;
        let after_paths = &after.indexes.references;
        let mut edits = Vec::new();
        for note in self.notes.iter() {
            let new_path = moved_path(&note.path, from, to);
            let mut edit: Option<(String, String)> = None;
            for link in note.references.iter().rev() {
                let Some(target) = before_paths.resolve(&note.path, &link.target, link.wiki) else {
                    continue;
                };
                let target = moved_path(&target, from, to);
                if after_paths
                    .resolve(&new_path, &link.target, link.wiki)
                    .as_ref()
                    == Some(&target)
                {
                    continue;
                }
                let mut replacement = crate::links::retarget(&new_path, &target, &link.target, link.wiki)
                    .ok_or_else(|| invalid("This destination cannot be addressed by wiki links; avoid # and ]] in its name"))?;
                if link.wiki
                    && replacement.starts_with('/')
                    && after_paths
                        .resolve(&new_path, &replacement[1..], true)
                        .as_ref()
                        == Some(&target)
                {
                    replacement.remove(0);
                }
                if edit.is_none() {
                    let baseline = self.read(&note.path)?;
                    // Link ranges belong to the scanned contents. A racing edit
                    // must not apply those ranges to different UTF-8 text.
                    if !note.matches_text(&baseline) {
                        return Err(invalid("A note changed while preparing the move; retry"));
                    }
                    edit = Some((baseline.clone(), baseline));
                }
                let (_, text) = edit.as_mut().unwrap();
                text.replace_range(link.destination.clone(), &replacement);
            }
            if let Some((baseline, text)) = edit {
                edits.push((note.path.clone(), baseline, text));
            }
        }
        // Preflight every file before any replacement. save() checks again at
        // each write and the rollback refuses to overwrite concurrent edits.
        for (path, baseline, _) in &edits {
            if self.read(path)? != *baseline {
                return Err(invalid("A note changed while preparing the move; retry"));
            }
        }
        self.commit_move(&edits, &source, &destination)?;
        // Install the committed snapshot without another fallible directory scan.
        let mut changed: HashMap<_, _> = edits
            .into_iter()
            .map(|(p, _, text)| (moved_path(&p, from, to), text))
            .collect();
        after.notes = Arc::new(
            after
                .notes
                .iter()
                .map(|n| {
                    changed
                        .remove(&n.path)
                        .map(|text| Arc::new(Note::new(n.path.clone(), &text, None)))
                        .unwrap_or_else(|| n.clone())
                })
                .collect(),
        );
        Arc::make_mut(&mut after.notes)
            .sort_by(|a, b| a.search_path.cmp(&b.search_path).then(a.path.cmp(&b.path)));
        Arc::make_mut(&mut after.folders).sort();
        Arc::make_mut(&mut after.files).sort();
        after.reindex();
        *self = after;
        Ok(())
    }
    pub(super) fn commit_move(
        &self,
        edits: &[(PathBuf, String, String)],
        source: &Path,
        destination: &Path,
    ) -> io::Result<()> {
        let mut written = 0;
        let result = (|| -> io::Result<()> {
            for (path, baseline, text) in edits {
                // Count the attempt: persistence can succeed before a directory
                // sync reports an error, so the failing write may need rollback.
                written += 1;
                self.save_file(path, baseline, text)
                    .map_err(io::Error::other)?;
            }
            // Atomic NOREPLACE prevents a racing writer's destination from
            // being overwritten, including an empty directory on Linux.
            let from = source.strip_prefix(&self.root).map_err(io::Error::other)?;
            let to = destination
                .strip_prefix(&self.root)
                .map_err(io::Error::other)?;
            let moved_notes = self
                .notes
                .iter()
                .filter(|note| note.path.starts_with(from))
                .map(|note| note.path.clone())
                .collect::<Vec<_>>();
            crate::history::move_entry(&self.root, from, to, &moved_notes)?;
            Ok(())
        })();
        if let Err(error) = result {
            let mut failures = Vec::new();
            for (path, old, new) in edits[..written].iter().rev() {
                if self.read(path).as_ref().is_ok_and(|text| text == old) {
                    continue;
                }
                if let Err(e) = self.save_file(path, new, old) {
                    failures.push(format!("{}: {e}", path.display()));
                }
            }
            return Err(if failures.is_empty() {
                error
            } else {
                io::Error::other(format!(
                    "{error}. Some link changes could not be rolled back: {}",
                    failures.join("; ")
                ))
            });
        }
        Ok(())
    }
}
