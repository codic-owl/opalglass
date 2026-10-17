use super::*;

impl Vault {
    pub fn resolve(&self, from: &Path, target: &str) -> Option<PathBuf> {
        let target = target.split('#').next().unwrap_or(target);
        if target.is_empty() {
            return Some(from.into());
        }
        let rooted = target.starts_with('/');
        let path = Self::note_path(target.strip_prefix('/').unwrap_or(target)).ok()?;
        if rooted {
            return self
                .indexes
                .paths
                .get(&path)
                .map(|&i| self.notes[i].path.clone());
        }
        let local = from.parent().unwrap_or(Path::new("")).join(&path);
        for candidate in [&local, &path] {
            if let Some(&index) = self.indexes.paths.get(candidate) {
                return Some(self.notes[index].path.clone());
            }
        }
        if path.components().count() > 1 {
            return None;
        }
        let stem = path.file_stem()?.to_string_lossy();
        self.indexes
            .titles
            .get(stem.as_ref())
            .copied()
            .flatten()
            .map(|index| self.notes[index].path.clone())
    }
    pub fn contains_path(&self, path: &Path) -> bool {
        self.indexes.paths.contains_key(path)
    }
    /// Resolve standard Markdown URLs, including percent escapes and `../`.
    pub fn resolve_attachment(&self, from: &Path, target: &str) -> Option<PathBuf> {
        self.indexes
            .references
            .resolve(from, target, true)
            .filter(|p| self.files.contains(p))
    }
    pub fn resolve_url(&self, from: &Path, target: &str) -> Option<PathBuf> {
        self.indexes.references.resolve(from, target, false)
    }
    /// None means this note has not been content-indexed yet.
    pub fn anchors(&self, path: &Path) -> Option<&crate::anchors::Index> {
        self.indexes
            .paths
            .get(path)
            .and_then(|&i| self.notes[i].anchors.as_ref())
    }
}
