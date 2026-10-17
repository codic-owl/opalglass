// Path resolution and incremental incoming-link indexing.
use super::*;

impl Vault {
    pub(super) fn reindex(&mut self) {
        self.reindex_cancellable(&AtomicBool::new(false)).unwrap();
    }
    pub(super) fn reindex_cancellable(&mut self, cancelled: &AtomicBool) -> io::Result<()> {
        let mut indexes = Indexes::default();
        for (index, note) in self.notes.iter().enumerate() {
            check_cancelled(cancelled)?;
            indexes.paths.insert(note.path.clone(), index);
            indexes
                .titles
                .entry(note.title.clone())
                .and_modify(|v| *v = None)
                .or_insert(Some(index));
        }
        indexes.references = crate::links::Resolver::new(
            self.notes
                .iter()
                .map(|n| n.path.clone())
                .chain(self.files.iter().cloned()),
        );
        // Resolve using the new path/title maps before building incoming links.
        self.indexes = Arc::new(indexes);
        let mut incoming: HashMap<PathBuf, Vec<usize>> = HashMap::new();
        for (index, note) in self.notes.iter().enumerate() {
            check_cancelled(cancelled)?;
            for target in self.backlink_targets(note, cancelled)? {
                incoming.entry(target).or_default().push(index);
            }
        }
        Arc::get_mut(&mut self.indexes).unwrap().incoming = incoming;
        Ok(())
    }
    pub(super) fn update_note(&mut self, path: &Path, text: &str) {
        // Revalidate a local write on the next scan. Sampling metadata after
        // replacement could accidentally cache a concurrent writer's timestamp
        // alongside our older text.
        let note = Arc::new(Note::new(path.into(), text, None));
        if let Some(&index) = self.indexes.paths.get(path) {
            // Saving an existing path cannot change name resolution or note
            // indices. Update only its outgoing edges, not the entire vault.
            let targets = |note: &Note| {
                self.backlink_targets(note, &AtomicBool::new(false))
                    .expect("a local metadata update is not cancelled")
            };
            let old = targets(&self.notes[index]);
            let new = targets(&note);
            Arc::make_mut(&mut self.notes)[index] = note;
            if old != new {
                let indexes = Arc::make_mut(&mut self.indexes);
                for target in old.difference(&new) {
                    if let Some(sources) = indexes.incoming.get_mut(target) {
                        sources.retain(|&source| source != index);
                        if sources.is_empty() {
                            indexes.incoming.remove(target);
                        }
                    }
                }
                for target in new.difference(&old) {
                    let sources = indexes.incoming.entry(target.clone()).or_default();
                    if let Err(position) = sources.binary_search(&index) {
                        sources.insert(position, index);
                    }
                }
            }
        } else {
            let notes = Arc::make_mut(&mut self.notes);
            notes.push(note);
            notes.sort_by(|a, b| a.search_path.cmp(&b.search_path).then(a.path.cmp(&b.path)));
            self.reindex();
        }
    }

    fn backlink_targets(
        &self,
        note: &Note,
        cancelled: &AtomicBool,
    ) -> io::Result<HashSet<PathBuf>> {
        let mut seen = HashSet::new();
        let mut targets = HashSet::new();
        for reference in &note.references {
            check_cancelled(cancelled)?;
            if !reference.used || !seen.insert((reference.target.as_str(), reference.wiki)) {
                continue;
            }
            let target = if reference.wiki {
                self.resolve(&note.path, &reference.target)
            } else {
                self.indexes
                    .references
                    .resolve(&note.path, &reference.target, false)
            };
            if let Some(target) = target
                && target != note.path
                && self.indexes.paths.contains_key(&target)
            {
                targets.insert(target);
            }
        }
        Ok(targets)
    }
}

#[cfg(test)]
mod tests;
