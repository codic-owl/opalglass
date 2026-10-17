use crate::markdown::WikiLink;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};

mod note;
pub use note::Note;
use note::Stamp;

#[derive(Debug, Default, Clone)]
struct Indexes {
    paths: HashMap<PathBuf, usize>,
    titles: HashMap<String, Option<usize>>,
    incoming: HashMap<PathBuf, Vec<usize>>,
    references: crate::links::Resolver,
}

#[derive(Debug, Clone)]
pub struct Vault {
    pub root: PathBuf,
    pub notes: Arc<Vec<Arc<Note>>>,
    pub folders: Arc<Vec<PathBuf>>,
    /// Visible non-Markdown files, indexed by path without loading their bodies.
    pub files: Arc<Vec<PathBuf>>,
    indexes: Arc<Indexes>,
    content_indexed: bool,
}

#[derive(Debug)]
pub enum SaveError {
    Conflict,
    Io(io::Error),
}
impl std::error::Error for SaveError {}
impl From<io::Error> for SaveError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict => write!(
                f,
                "The note changed on disk. Your edits are still open. Save a copy or reload the disk version."
            ),
            Self::Io(error) => error.fmt(f),
        }
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn check_cancelled(cancelled: &AtomicBool) -> io::Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Vault loading cancelled",
        ))
    } else {
        Ok(())
    }
}

impl Vault {
    pub fn open(root: impl AsRef<Path>) -> io::Result<Self> {
        Self::scan_initial(root, true, &AtomicBool::new(false))
    }
    /// List paths and build the resolver without reading any note bodies.
    /// Individual notes can be opened immediately; search bodies/backlinks are
    /// filled by a subsequent background refresh.
    pub fn discover(root: impl AsRef<Path>, cancelled: &AtomicBool) -> io::Result<Self> {
        Self::scan_initial(root, false, cancelled)
    }
    pub fn content_indexed(&self) -> bool {
        self.content_indexed
    }
    fn scan_initial(
        root: impl AsRef<Path>,
        contents: bool,
        cancelled: &AtomicBool,
    ) -> io::Result<Self> {
        check_cancelled(cancelled)?;
        let root = root.as_ref().canonicalize()?;
        if !root.is_dir() {
            return Err(invalid("Choose a directory for the vault"));
        }
        let mut vault = Self {
            root,
            notes: Arc::default(),
            folders: Arc::default(),
            files: Arc::default(),
            indexes: Arc::default(),
            content_indexed: false,
        };
        vault.scan(contents, cancelled, &mut |_| {})?;
        Ok(vault)
    }
    /// Reuse unchanged metadata and parsed links; bodies are read one at a time
    /// and discarded after extracting links and a content fingerprint.
    /// Returns false when no files changed, so the UI need not redraw.
    pub fn refresh(&mut self) -> io::Result<bool> {
        self.refresh_cancellable(&AtomicBool::new(false))
    }
    pub fn refresh_cancellable(&mut self, cancelled: &AtomicBool) -> io::Result<bool> {
        self.refresh_with_progress(cancelled, |_| {})
    }
    pub fn refresh_with_progress(
        &mut self,
        cancelled: &AtomicBool,
        mut progress: impl FnMut(usize),
    ) -> io::Result<bool> {
        self.scan(true, cancelled, &mut progress)
    }
    fn scan(
        &mut self,
        contents: bool,
        cancelled: &AtomicBool,
        progress: &mut impl FnMut(usize),
    ) -> io::Result<bool> {
        #[derive(Default)]
        struct Entries {
            notes: Vec<Arc<Note>>,
            folders: Vec<PathBuf>,
            files: Vec<PathBuf>,
        }
        fn visit(
            vault: &Vault,
            dir: &Path,
            entries: &mut Entries,
            contents: bool,
            cancelled: &AtomicBool,
            progress: &mut impl FnMut(usize),
        ) -> io::Result<()> {
            for entry in fs::read_dir(dir)? {
                check_cancelled(cancelled)?;
                let entry = entry?;
                let kind = entry.file_type()?;
                if kind.is_symlink() || entry.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                let path = entry.path();
                if kind.is_dir() {
                    entries
                        .folders
                        .push(path.strip_prefix(&vault.root).unwrap().into());
                    visit(vault, &path, entries, contents, cancelled, progress)?;
                } else if kind.is_file()
                    && path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
                {
                    let relative = path.strip_prefix(&vault.root).unwrap();
                    if !contents {
                        entries
                            .notes
                            .push(Arc::new(Note::discovered(relative.into())));
                        continue;
                    }
                    let stamp = Stamp::new(&entry.metadata()?);
                    if let Some(&index) = vault.indexes.paths.get(relative)
                        && vault.notes[index].stamp.as_ref() == Some(&stamp)
                    {
                        entries.notes.push(vault.notes[index].clone());
                    } else {
                        let text = fs::read_to_string(&path)?;
                        entries.notes.push(Arc::new(Note::new(
                            relative.into(),
                            &text,
                            Some(stamp),
                        )));
                    }
                    progress(entries.notes.len());
                } else if kind.is_file() {
                    entries
                        .files
                        .push(path.strip_prefix(&vault.root).unwrap().into());
                }
            }
            Ok(())
        }
        let mut entries = Entries::default();
        check_cancelled(cancelled)?;
        visit(
            self,
            &self.root,
            &mut entries,
            contents,
            cancelled,
            progress,
        )?;
        let Entries {
            mut notes,
            mut folders,
            mut files,
        } = entries;
        folders.sort();
        files.sort();
        notes.sort_by(|a, b| a.search_path.cmp(&b.search_path).then(a.path.cmp(&b.path)));
        let changed = folders != *self.folders
            || files != *self.files
            || notes.len() != self.notes.len()
            || notes
                .iter()
                .zip(self.notes.iter())
                .any(|(a, b)| !Arc::ptr_eq(a, b));
        if changed || self.content_indexed != (contents || notes.is_empty()) {
            // Build off to the side so cancellation never installs a partial
            // resolver, incomplete backlink index, or half-read snapshot.
            let mut next = self.clone();
            next.content_indexed = contents || notes.is_empty();
            next.notes = Arc::new(notes);
            next.folders = Arc::new(folders);
            next.files = Arc::new(files);
            next.reindex_cancellable(cancelled)?;
            check_cancelled(cancelled)?;
            *self = next;
            return Ok(true);
        }
        Ok(changed)
    }
    /// Paths must stay relative, visible and portable between Windows and Unix.
    pub fn note_path(name: &str) -> io::Result<PathBuf> {
        let name = name.trim();
        if name.is_empty()
            || name.contains(['\\', ':', '*', '?', '"', '<', '>', '|', '\0'])
            || name.chars().any(char::is_control)
        {
            return Err(invalid(
                "Use a note name without reserved filename characters",
            ));
        }
        let mut path = PathBuf::from(name);
        for component in path.components() {
            let Component::Normal(part) = component else {
                return Err(invalid("Note paths must stay inside the vault"));
            };
            let part = part.to_string_lossy();
            let stem = part.split('.').next().unwrap_or("").to_uppercase();
            if part.starts_with('.')
                || part.ends_with(['.', ' '])
                || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || (stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.as_bytes()[3].is_ascii_digit())
            {
                return Err(invalid("That name is hidden or reserved on Windows"));
            }
        }
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            path.as_mut_os_string().push(".md");
        }
        Ok(path)
    }
    fn checked(&self, relative: &Path) -> io::Result<PathBuf> {
        // Use path components, not the native separator: Windows paths contain
        // backslashes even though the user-facing note syntax uses forward slashes.
        let parts = relative
            .components()
            .map(|part| match part {
                Component::Normal(name) => Ok(name.to_string_lossy()),
                _ => Err(invalid("Note paths must stay inside the vault")),
            })
            .collect::<io::Result<Vec<_>>>()?;
        if Self::note_path(&parts.join("/"))? != relative {
            return Err(invalid("Expected a Markdown note path"));
        }
        self.checked_entry(relative)
    }
    /// A visible vault-relative path, with the same confinement rules as notes.
    pub fn folder_path(name: &str) -> io::Result<PathBuf> {
        // Validate components without changing the final extension.
        Self::note_path(name)?;
        Ok(PathBuf::from(name.trim()))
    }
    fn checked_entry(&self, path: &Path) -> io::Result<PathBuf> {
        if Self::folder_path(&path.to_string_lossy())? != path {
            return Err(invalid("Invalid vault path"));
        }
        let mut full = self.root.clone();
        for component in path.components() {
            full.push(component);
            match fs::symlink_metadata(&full) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(invalid("Symlinks are not followed inside vaults"));
                }
                Ok(_) => (),
                Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                Err(e) => return Err(e),
            }
        }
        Ok(full)
    }
    fn trash_directory(&self, create: bool) -> io::Result<PathBuf> {
        let dir = self.root.join(".trash");
        if create {
            match fs::create_dir(&dir) {
                Ok(()) => (),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(e),
            }
        }
        let metadata = fs::symlink_metadata(&dir)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(invalid("The vault trash must be a real directory"));
        }
        Ok(dir)
    }
    /// Move an entire entry, including attachments, without following links or
    /// rewriting references. Each operation keeps its original relative path.
    pub fn trash_entry(&mut self, path: &Path) -> io::Result<()> {
        let source = self.checked_entry(path)?;
        let metadata = fs::symlink_metadata(&source)?;
        if !metadata.is_file() && !metadata.is_dir() {
            return Err(invalid("Only files and folders can be moved to trash"));
        }
        let dir = self.trash_directory(true)?;
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let entry = tempfile::Builder::new()
            .prefix(&format!("{stamp:039}-"))
            .tempdir_in(&dir)?;
        crate::state::write_json(&entry.path().join("original.json"), &path)?;
        fs::rename(&source, entry.path().join("content"))?;
        // Keep the payload even when later refreshes fail.
        let _ = entry.keep();
        self.refresh()?;
        Ok(())
    }
    /// Restore the newest remaining deletion. Existing paths are never replaced.
    pub fn restore_last_trashed(&mut self) -> io::Result<PathBuf> {
        let dir = self.trash_directory(false)?;
        let mut entries = fs::read_dir(&dir)?.collect::<io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.file_name()));
        for entry in entries {
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let payload = entry.path().join("content");
            let metadata = match fs::symlink_metadata(&payload) {
                Ok(metadata) => metadata,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            if metadata.file_type().is_symlink() {
                return Err(invalid("Trash contains a symlink"));
            }
            let manifest = entry.path().join("original.json");
            if !fs::symlink_metadata(&manifest)?.is_file() {
                return Err(invalid("Invalid trash record"));
            }
            let relative: PathBuf = serde_json::from_slice(&fs::read(manifest)?)?;
            let destination = self.checked_entry(&relative)?;
            let parent = destination
                .parent()
                .ok_or_else(|| invalid("Invalid original location"))?;
            fs::create_dir_all(parent)?;
            self.check_destination(&relative, None)?;
            // A destination may appear after the collision check. Restore must
            // preserve that file and leave the trash payload available to retry.
            crate::platform::rename_new(&payload, &destination)?;
            // Leave only harmless metadata if cleanup fails; payload-less
            // records are ignored on the next restore.
            let _ = fs::remove_dir_all(entry.path());
            self.refresh()?;
            return Ok(relative);
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No deleted files to restore",
        ))
    }
    pub fn create_folder(&mut self, name: &str) -> io::Result<PathBuf> {
        let path = Self::folder_path(name)?;
        let full = self.checked_entry(&path)?;
        self.check_destination(&path, None)?;
        fs::create_dir(&full)?;
        let folders = Arc::make_mut(&mut self.folders);
        folders.push(path.clone());
        folders.sort();
        Ok(path)
    }
    fn check_destination(&self, path: &Path, excluding: Option<&Path>) -> io::Result<()> {
        let full = self.checked_entry(path)?;
        let parent = full
            .parent()
            .ok_or_else(|| invalid("Invalid destination"))?;
        let name = full.file_name().unwrap().to_string_lossy().to_lowercase();
        for entry in fs::read_dir(parent)? {
            let entry = entry?;
            if excluding.is_some_and(|p| self.root.join(p) == entry.path()) {
                continue;
            }
            if entry.file_name().to_string_lossy().to_lowercase() == name {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "The destination already exists",
                ));
            }
        }
        Ok(())
    }
    pub fn read(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(self.checked(path)?)
    }
    pub fn create(&mut self, name: &str, text: &str) -> io::Result<PathBuf> {
        let path = Self::note_path(name)?;
        // Avoid case-only duplicates that would break on Windows/macOS.
        if self.notes.iter().any(|n| {
            n.path.to_string_lossy().to_lowercase() == path.to_string_lossy().to_lowercase()
        }) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "A note with that name already exists",
            ));
        }
        let full = self.checked(&path)?;
        fs::create_dir_all(full.parent().unwrap())?;
        let mut temp = tempfile::NamedTempFile::new_in(full.parent().unwrap())?;
        temp.write_all(text.as_bytes())?;
        temp.as_file().sync_all()?;
        temp.persist_noclobber(&full).map_err(|e| e.error)?;
        self.update_note(&path, text);
        let mut parent = path.parent();
        let folders = Arc::make_mut(&mut self.folders);
        while let Some(p) = parent.filter(|p| !p.as_os_str().is_empty()) {
            if !folders.contains(&p.to_path_buf()) {
                folders.push(p.into());
            }
            parent = p.parent();
        }
        folders.sort();
        Ok(path)
    }
    pub fn save(&mut self, path: &Path, baseline: &str, text: &str) -> Result<(), SaveError> {
        self.save_file(path, baseline, text)?;
        self.update_note(path, text);
        Ok(())
    }
    fn save_file(&self, path: &Path, baseline: &str, text: &str) -> Result<(), SaveError> {
        let full = self.checked(path)?;
        let disk = fs::read_to_string(&full).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                SaveError::Conflict
            } else {
                SaveError::Io(e)
            }
        })?;
        if disk != baseline {
            return Err(SaveError::Conflict);
        }
        if disk != text {
            crate::history::capture(&self.root, path, &disk)?;
        }
        let mut temp = tempfile::NamedTempFile::new_in(full.parent().unwrap())?;
        temp.as_file()
            .set_permissions(fs::metadata(&full)?.permissions())?;
        temp.write_all(text.as_bytes())?;
        temp.as_file().sync_all()?;
        // Recheck immediately before replacement. This is optimistic concurrency,
        // not a filesystem-wide lock against other editors.
        if fs::read_to_string(&full)? != baseline {
            return Err(SaveError::Conflict);
        }
        temp.persist(&full).map_err(|e| e.error)?;
        #[cfg(unix)]
        fs::File::open(full.parent().unwrap())?.sync_all()?;
        Ok(())
    }
    pub fn file_path(&self, path: &Path) -> io::Result<PathBuf> {
        if !self.files.contains(&path.to_owned()) {
            return Err(invalid("Attachment is not in this vault"));
        }
        let full = self.checked_entry(path)?;
        if !fs::metadata(&full)?.is_file() {
            return Err(invalid("Attachment is not a file"));
        }
        Ok(full)
    }
    pub fn backlinks(&self, path: &Path) -> Vec<&Note> {
        self.indexes
            .incoming
            .get(path)
            .into_iter()
            .flatten()
            .map(|&index| self.notes[index].as_ref())
            .collect()
    }
    /// Filename/path filtering only. Full-text queries belong to SearchIndex.
    pub fn search(&self, query: &str) -> Vec<&Note> {
        self.search_indices(query)
            .into_iter()
            .map(|index| self.notes[index].as_ref())
            .collect()
    }
    pub fn search_indices(&self, query: &str) -> Vec<usize> {
        self.search_indices_cancellable(query, &AtomicBool::new(false))
    }
    pub fn search_indices_cancellable(&self, query: &str, cancelled: &AtomicBool) -> Vec<usize> {
        let query = query.to_lowercase();
        self.notes
            .iter()
            .enumerate()
            .take_while(|_| !cancelled.load(Ordering::Relaxed))
            .filter(|(_, n)| query.is_empty() || n.search_path.contains(&query))
            .map(|(index, _)| index)
            .collect()
    }
}

/// Map an entry or descendant after a folder/file move.
pub fn moved_path(path: &Path, from: &Path, to: &Path) -> PathBuf {
    path.strip_prefix(from)
        .map(|suffix| {
            if suffix.as_os_str().is_empty() {
                to.into()
            } else {
                to.join(suffix)
            }
        })
        .unwrap_or_else(|_| path.into())
}

#[cfg(test)]
mod tests;

mod moves;

mod index;

mod resolve;
