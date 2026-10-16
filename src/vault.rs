use crate::markdown::{WikiLink, wiki_links};
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

#[derive(Debug, Clone)]
pub struct Note {
    pub path: PathBuf,
    pub title: String,
    pub links: Vec<WikiLink>,
    content_hash: [u8; 32],
    search_path: String,
    stamp: Option<Stamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    #[cfg(unix)]
    changed: (i64, i64),
}

impl Stamp {
    fn new(meta: &fs::Metadata) -> Self {
        Self {
            len: meta.len(),
            modified: meta.modified().ok(),
            created: meta.created().ok(),
            #[cfg(unix)]
            changed: {
                use std::os::unix::fs::MetadataExt;
                (meta.ctime(), meta.ctime_nsec())
            },
        }
    }
}

impl Note {
    /// Compare saved contents without keeping another copy of the note body.
    /// Writes still compare the full disk text immediately before replacement.
    pub fn matches_text(&self, text: &str) -> bool {
        self.content_hash == <[u8; 32]>::from(Sha256::digest(text.as_bytes()))
    }
    pub fn modified(&self) -> Option<SystemTime> {
        self.stamp.as_ref().and_then(|stamp| stamp.modified)
    }
    pub fn created(&self) -> Option<SystemTime> {
        self.stamp.as_ref().and_then(|stamp| stamp.created)
    }
    fn new(path: PathBuf, text: &str, stamp: Option<Stamp>) -> Self {
        Self {
            title: path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            search_path: path.to_string_lossy().to_lowercase(),
            links: wiki_links(text),
            path,
            content_hash: Sha256::digest(text.as_bytes()).into(),
            stamp,
        }
    }
}

#[derive(Debug, Default, Clone)]
struct Indexes {
    paths: HashMap<PathBuf, usize>,
    titles: HashMap<String, Option<usize>>,
    incoming: HashMap<PathBuf, Vec<usize>>,
}

#[derive(Debug, Clone)]
pub struct Vault {
    pub root: PathBuf,
    pub notes: Arc<Vec<Arc<Note>>>,
    pub folders: Arc<Vec<PathBuf>>,
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
        fn visit(
            vault: &Vault,
            dir: &Path,
            notes: &mut Vec<Arc<Note>>,
            folders: &mut Vec<PathBuf>,
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
                    folders.push(path.strip_prefix(&vault.root).unwrap().into());
                    visit(vault, &path, notes, folders, contents, cancelled, progress)?;
                } else if kind.is_file()
                    && path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
                {
                    let relative = path.strip_prefix(&vault.root).unwrap();
                    if !contents {
                        notes.push(Arc::new(Note::new(relative.into(), "", None)));
                        continue;
                    }
                    let stamp = Stamp::new(&entry.metadata()?);
                    if let Some(&index) = vault.indexes.paths.get(relative)
                        && vault.notes[index].stamp.as_ref() == Some(&stamp)
                    {
                        notes.push(vault.notes[index].clone());
                    } else {
                        let text = fs::read_to_string(&path)?;
                        notes.push(Arc::new(Note::new(relative.into(), &text, Some(stamp))));
                    }
                    progress(notes.len());
                }
            }
            Ok(())
        }
        let mut notes = Vec::new();
        let mut folders = Vec::new();
        check_cancelled(cancelled)?;
        visit(
            self,
            &self.root,
            &mut notes,
            &mut folders,
            contents,
            cancelled,
            progress,
        )?;
        folders.sort();
        notes.sort_by(|a, b| a.search_path.cmp(&b.search_path).then(a.path.cmp(&b.path)));
        let changed = folders != *self.folders
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
            next.reindex_cancellable(cancelled)?;
            check_cancelled(cancelled)?;
            *self = next;
            return Ok(true);
        }
        Ok(changed)
    }
    fn reindex(&mut self) {
        self.reindex_cancellable(&AtomicBool::new(false)).unwrap();
    }
    fn reindex_cancellable(&mut self, cancelled: &AtomicBool) -> io::Result<()> {
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
        // Resolve using the new path/title maps before building incoming links.
        self.indexes = Arc::new(indexes);
        let mut incoming: HashMap<PathBuf, Vec<usize>> = HashMap::new();
        for (index, note) in self.notes.iter().enumerate() {
            check_cancelled(cancelled)?;
            let mut seen = HashSet::new();
            for link in &note.links {
                check_cancelled(cancelled)?;
                if !seen.insert(link.target.as_str()) {
                    continue;
                }
                if let Some(target) = self.resolve(&note.path, &link.target)
                    && target != note.path
                {
                    let sources = incoming.entry(target).or_default();
                    if sources.last() != Some(&index) {
                        sources.push(index);
                    }
                }
            }
        }
        Arc::get_mut(&mut self.indexes).unwrap().incoming = incoming;
        Ok(())
    }
    fn update_note(&mut self, path: &Path, text: &str) {
        // Revalidate a local write on the next scan. Sampling metadata after
        // replacement could accidentally cache a concurrent writer's timestamp
        // alongside our older text.
        let note = Arc::new(Note::new(path.into(), text, None));
        if let Some(&index) = self.indexes.paths.get(path) {
            // Saving an existing path cannot change name resolution or note
            // indices. Update only its outgoing edges, not the entire vault.
            let targets = |note: &Note| {
                let mut seen = HashSet::new();
                note.links
                    .iter()
                    .filter(|link| seen.insert(link.target.as_str()))
                    .filter_map(|link| self.resolve(&note.path, &link.target))
                    .filter(|target| target != &note.path)
                    .collect::<HashSet<_>>()
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
            fs::rename(&payload, &destination)?;
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
    /// Move an entry and preserve the resolved targets of wiki links. Callers
    /// save their open buffers first. All work may run on a background thread.
    pub fn move_entry(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        let source = self.checked_entry(from)?;
        let destination = self.checked_entry(to)?;
        if to == from || to.starts_with(from) {
            return Err(invalid("Choose a different location outside this folder"));
        }
        self.check_destination(to, Some(from))?;
        let is_folder = fs::metadata(&source)?.is_dir();
        if !is_folder && Self::note_path(&to.to_string_lossy())? != to {
            return Err(invalid("Notes must keep their .md extension"));
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
        after.reindex();
        let mut edits = Vec::new();
        for note in self.notes.iter() {
            let new_path = moved_path(&note.path, from, to);
            let mut edit: Option<(String, String)> = None;
            for link in note.links.iter().rev() {
                let Some(target) = self.resolve(&note.path, &link.target) else {
                    continue;
                };
                let target = moved_path(&target, from, to);
                if after.resolve(&new_path, &link.target).as_ref() == Some(&target) {
                    continue;
                }
                let (old_target, heading) = link
                    .target
                    .split_once('#')
                    .map_or((link.target.as_str(), ""), |(a, b)| (a, b));
                let target_path = if old_target.ends_with(".md") {
                    target.clone()
                } else {
                    target.with_extension("")
                };
                let mut replacement = target_path.to_string_lossy().replace('\\', "/");
                if after.resolve(&new_path, &replacement).as_ref() != Some(&target) {
                    replacement.insert(0, '/');
                }
                if replacement.contains("]]")
                    || after.resolve(&new_path, &replacement).as_ref() != Some(&target)
                {
                    return Err(invalid(
                        "This destination cannot be addressed by wiki links; avoid # and ]] in its name",
                    ));
                }
                if link.target.contains('#') {
                    replacement.push('#');
                    replacement.push_str(heading);
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
                let (baseline, text) = edit.as_mut().unwrap();
                let inner = &baseline[link.range.start + 2..link.range.end - 2];
                let alias = inner.find('|').map(|i| &inner[i..]).unwrap_or("");
                text.replace_range(link.range.clone(), &format!("[[{replacement}{alias}]]"));
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
        after.reindex();
        *self = after;
        Ok(())
    }
    fn commit_move(
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
            rustix::fs::renameat_with(
                rustix::fs::CWD,
                source,
                rustix::fs::CWD,
                destination,
                rustix::fs::RenameFlags::NOREPLACE,
            )
            .map_err(io::Error::from)?;
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
mod tests {
    use super::*;
    #[test]
    fn trash_preserves_folders_attachments_links_and_restores_without_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("Folder")).unwrap();
        fs::write(dir.path().join("Folder/A.md"), "saved edits").unwrap();
        fs::write(dir.path().join("Folder/photo.bin"), [0, 1, 255]).unwrap();
        fs::write(dir.path().join("B.md"), "[[Folder/A]]").unwrap();
        let mut vault = Vault::open(dir.path()).unwrap();
        vault.trash_entry(Path::new("Folder")).unwrap();
        assert!(!dir.path().join("Folder").exists());
        assert_eq!(vault.read(Path::new("B.md")).unwrap(), "[[Folder/A]]");
        assert!(vault.resolve(Path::new("B.md"), "Folder/A").is_none());
        assert!(vault.notes.iter().all(|n| !n.path.starts_with(".trash")));
        // Trash survives a new Vault instance and refuses an occupied target.
        let mut vault = Vault::open(dir.path()).unwrap();
        fs::create_dir(dir.path().join("Folder")).unwrap();
        assert!(vault.restore_last_trashed().is_err());
        fs::remove_dir(dir.path().join("Folder")).unwrap();
        assert_eq!(
            vault.restore_last_trashed().unwrap(),
            PathBuf::from("Folder")
        );
        assert_eq!(
            fs::read(dir.path().join("Folder/photo.bin")).unwrap(),
            vec![0, 1, 255]
        );
        assert_eq!(vault.read(Path::new("Folder/A.md")).unwrap(), "saved edits");
        assert!(vault.resolve(Path::new("B.md"), "Folder/A").is_some());
        assert!(vault.restore_last_trashed().is_err());
        for invalid in ["", "..", "../outside", ".trash", "/tmp"] {
            assert!(vault.trash_entry(Path::new(invalid)).is_err());
        }
    }
    #[cfg(unix)]
    #[test]
    fn trash_rejects_symlinks_and_malicious_restore_records() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("A.md"), "note").unwrap();
        symlink(outside.path(), dir.path().join(".trash")).unwrap();
        let mut vault = Vault::open(dir.path()).unwrap();
        assert!(vault.trash_entry(Path::new("A.md")).is_err());
        assert!(dir.path().join("A.md").exists());
        fs::remove_file(dir.path().join(".trash")).unwrap();
        vault.trash_entry(Path::new("A.md")).unwrap();
        let entry = fs::read_dir(dir.path().join(".trash"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::write(entry.join("original.json"), "\"../outside.md\"").unwrap();
        assert!(vault.restore_last_trashed().is_err());
        assert!(entry.join("content").exists());
    }

    #[test]
    fn updating_existing_note_changes_only_its_backlinks_and_preserves_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in [
            ("A", "[[Target]]"),
            ("B", "[[Other]]"),
            ("C", "[[Target]]"),
            ("Other", ""),
            ("Target", ""),
        ] {
            fs::write(dir.path().join(format!("{name}.md")), text).unwrap();
        }
        let mut vault = Vault::open(dir.path()).unwrap();
        let snapshot = vault.clone();
        vault.update_note(
            Path::new("B.md"),
            "[[Target]] [[Target|alias]] [[B]] [[missing]]",
        );
        let sources = |vault: &Vault, target| {
            vault
                .backlinks(Path::new(target))
                .iter()
                .map(|n| n.path.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            sources(&vault, "Target.md"),
            ["A.md", "B.md", "C.md"].map(PathBuf::from)
        );
        assert!(sources(&vault, "Other.md").is_empty());
        assert!(sources(&vault, "B.md").is_empty());
        assert_eq!(
            sources(&snapshot, "Target.md"),
            ["A.md", "C.md"].map(PathBuf::from)
        );
        assert_eq!(sources(&snapshot, "Other.md"), [PathBuf::from("B.md")]);
        let indexes = vault.indexes.clone();
        vault.update_note(Path::new("B.md"), "Edited [[Target#heading]]");
        assert!(Arc::ptr_eq(&indexes, &vault.indexes));
        vault.update_note(Path::new("B.md"), "removed links");
        assert_eq!(
            sources(&vault, "Target.md"),
            ["A.md", "C.md"].map(PathBuf::from)
        );
    }
    #[test]
    fn discovery_resolves_paths_without_reading_contents_then_indexes_search_and_backlinks() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("Folder/Empty")).unwrap();
        fs::write(
            dir.path().join("Folder/Source.md"),
            "[[Target]] [[Target]] needle in body",
        )
        .unwrap();
        fs::write(dir.path().join("Folder/Target.md"), "# Target").unwrap();
        let mut vault = Vault::discover(dir.path(), &AtomicBool::new(false)).unwrap();
        assert!(!vault.content_indexed());
        assert!(
            vault
                .notes
                .iter()
                .all(|n| n.matches_text("") && n.links.is_empty())
        );
        assert!(vault.folders.contains(&PathBuf::from("Folder/Empty")));
        assert_eq!(
            vault.resolve(Path::new("Folder/Source.md"), "Target"),
            Some("Folder/Target.md".into())
        );
        assert_eq!(vault.search("Source").len(), 1);
        assert!(vault.search("needle in body").is_empty());
        assert_eq!(
            vault.read(Path::new("Folder/Target.md")).unwrap(),
            "# Target"
        );
        let mut progress = Vec::new();
        assert!(
            vault
                .refresh_with_progress(&AtomicBool::new(false), |count| progress.push(count))
                .unwrap()
        );
        assert_eq!(progress, [1, 2]);
        assert!(vault.content_indexed());
        assert!(vault.search("needle in body").is_empty());
        assert!(
            vault
                .notes
                .iter()
                .any(|n| n.matches_text("[[Target]] [[Target]] needle in body"))
        );
        assert_eq!(vault.backlinks(Path::new("Folder/Target.md")).len(), 1);
        assert!(!vault.refresh().unwrap());
    }
    #[test]
    fn discovery_survives_invalid_note_text_and_failed_indexing_preserves_inventory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Broken.md"), [0xff, 0xfe]).unwrap();
        let mut vault = Vault::discover(dir.path(), &AtomicBool::new(false)).unwrap();
        let before = vault.clone();
        assert_eq!(vault.notes.len(), 1);
        assert!(vault.refresh().is_err());
        assert!(!vault.content_indexed());
        assert!(Arc::ptr_eq(&vault.notes, &before.notes));
        assert!(Arc::ptr_eq(&vault.indexes, &before.indexes));
        fs::write(dir.path().join("Broken.md"), "fixed").unwrap();
        assert!(vault.refresh().unwrap());
        assert!(vault.content_indexed());
    }
    #[test]
    fn cancelled_discovery_and_indexing_do_not_install_partial_state() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("A.md"), "body").unwrap();
        let cancelled = AtomicBool::new(true);
        assert_eq!(
            Vault::discover(dir.path(), &cancelled).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        let mut vault = Vault::discover(dir.path(), &AtomicBool::new(false)).unwrap();
        let before = vault.clone();
        assert_eq!(
            vault.refresh_cancellable(&cancelled).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        assert!(Arc::ptr_eq(&vault.notes, &before.notes));
        assert!(Arc::ptr_eq(&vault.indexes, &before.indexes));
        assert!(!vault.content_indexed());
    }
    #[test]
    fn saves_and_moves_from_discovered_vault_preserve_unindexed_links() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("A.md"), "[[B]]\r\n").unwrap();
        fs::write(dir.path().join("B.md"), "target").unwrap();
        fs::write(dir.path().join("C.md"), "[[B|Alias]]").unwrap();
        let mut vault = Vault::discover(dir.path(), &AtomicBool::new(false)).unwrap();
        let baseline = vault.read(Path::new("A.md")).unwrap();
        vault
            .save(Path::new("A.md"), &baseline, "edited [[B]]\r\n")
            .unwrap();
        assert!(!vault.content_indexed());
        vault
            .move_entry(Path::new("B.md"), Path::new("Renamed.md"))
            .unwrap();
        assert!(vault.content_indexed());
        assert_eq!(
            fs::read_to_string(dir.path().join("A.md")).unwrap(),
            "edited [[Renamed]]\r\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("C.md")).unwrap(),
            "[[Renamed|Alias]]"
        );
    }
    #[test]
    fn rewrite_refuses_unaddressable_destination_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Note.md"), "").unwrap();
        fs::write(dir.path().join("Index.md"), "[[Note]]").unwrap();
        let mut vault = Vault::open(dir.path()).unwrap();
        for name in ["New#Heading.md", "New]]Name.md"] {
            assert!(
                vault
                    .move_entry(Path::new("Note.md"), Path::new(name))
                    .is_err()
            );
            assert!(dir.path().join("Note.md").exists());
            assert_eq!(vault.read(Path::new("Index.md")).unwrap(), "[[Note]]");
        }
    }
    #[test]
    fn failed_link_write_rolls_back_previous_edits_and_preserves_external_change() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Note.md"), "source").unwrap();
        fs::write(dir.path().join("A.md"), "[[Note]]").unwrap();
        fs::write(dir.path().join("B.md"), "external").unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        assert!(
            vault
                .commit_move(
                    &[
                        ("A.md".into(), "[[Note]]".into(), "[[Moved]]".into()),
                        ("B.md".into(), "[[Note]]".into(), "[[Moved]]".into())
                    ],
                    &dir.path().join("Note.md"),
                    &dir.path().join("Moved.md")
                )
                .is_err()
        );
        assert_eq!(vault.read(Path::new("A.md")).unwrap(), "[[Note]]");
        assert_eq!(vault.read(Path::new("B.md")).unwrap(), "external");
        assert!(dir.path().join("Note.md").exists());
    }
    #[test]
    fn failed_rename_rolls_back_link_writes_and_never_clobbers_destination() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Note.md"), "source").unwrap();
        fs::write(dir.path().join("Existing.md"), "destination").unwrap();
        fs::write(dir.path().join("Index.md"), "[[Note]]").unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        // Simulate a destination appearing after preflight: link writes have
        // already happened when the atomic NOREPLACE rename rejects it.
        assert!(
            vault
                .commit_move(
                    &[("Index.md".into(), "[[Note]]".into(), "[[Existing]]".into())],
                    &dir.path().join("Note.md"),
                    &dir.path().join("Existing.md")
                )
                .is_err()
        );
        assert_eq!(vault.read(Path::new("Index.md")).unwrap(), "[[Note]]");
        assert_eq!(vault.read(Path::new("Note.md")).unwrap(), "source");
        assert_eq!(vault.read(Path::new("Existing.md")).unwrap(), "destination");
    }
    #[test]
    fn rename_supports_case_changes_and_updates_links() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Note.md"), "").unwrap();
        fs::write(dir.path().join("Index.md"), "[[Note]]").unwrap();
        let mut vault = Vault::open(dir.path()).unwrap();
        vault
            .move_entry(Path::new("Note.md"), Path::new("note.md"))
            .unwrap();
        assert_eq!(vault.read(Path::new("Index.md")).unwrap(), "[[note]]");
        assert!(!dir.path().join("Note.md").exists());
    }
    #[test]
    fn move_folder_preserves_incoming_outgoing_aliases_headings_and_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("Old/Empty")).unwrap();
        fs::create_dir(dir.path().join("New")).unwrap();
        fs::write(
            dir.path().join("Old/Note.md"),
            "[[Sibling#Heading|local]]\r\n[[#Self]]\r\n[[Missing]]",
        )
        .unwrap();
        fs::write(dir.path().join("Old/Sibling.md"), "# Heading").unwrap();
        fs::write(
            dir.path().join("Index.md"),
            "![[Old/Note#Self|Alias 😀]]\r\n`[[Old/Note]]`\r\n```\r\n[[Old/Note]]\r\n```\r\n",
        )
        .unwrap();
        let mut vault = Vault::open(dir.path()).unwrap();
        vault
            .move_entry(Path::new("Old"), Path::new("New/Renamed"))
            .unwrap();
        assert!(!dir.path().join("Old").exists());
        assert!(dir.path().join("New/Renamed/Empty").is_dir());
        assert_eq!(
            vault.read(Path::new("Index.md")).unwrap(),
            "![[New/Renamed/Note#Self|Alias 😀]]\r\n`[[Old/Note]]`\r\n```\r\n[[Old/Note]]\r\n```\r\n"
        );
        assert_eq!(
            vault.read(Path::new("New/Renamed/Note.md")).unwrap(),
            "[[Sibling#Heading|local]]\r\n[[#Self]]\r\n[[Missing]]"
        );
        assert_eq!(
            vault.resolve(Path::new("New/Renamed/Note.md"), "Sibling#Heading"),
            Some("New/Renamed/Sibling.md".into())
        );
        assert_eq!(vault.backlinks(Path::new("New/Renamed/Note.md")).len(), 1);
        assert!(vault.folders.contains(&PathBuf::from("New/Renamed/Empty")));
        assert_eq!(vault.search("Renamed").len(), 2);
    }
    #[test]
    fn move_note_preserves_local_targets_despite_destination_shadowing() {
        let dir = tempfile::tempdir().unwrap();
        for path in ["A", "B/A"] {
            fs::create_dir_all(dir.path().join(path)).unwrap();
        }
        for (path, text) in [
            ("A/Note.md", "[[Sibling]] [[Other]]"),
            ("A/Sibling.md", ""),
            ("B/Sibling.md", ""),
            ("B/A/Sibling.md", ""),
            ("Other.md", ""),
            ("Index.md", "[[Note|Keep label]]"),
        ] {
            fs::write(dir.path().join(path), text).unwrap();
        }
        let mut vault = Vault::open(dir.path()).unwrap();
        vault
            .move_entry(Path::new("A/Note.md"), Path::new("B/Renamed.md"))
            .unwrap();
        assert_eq!(
            vault.read(Path::new("B/Renamed.md")).unwrap(),
            "[[/A/Sibling]] [[Other]]"
        );
        assert_eq!(
            vault.read(Path::new("Index.md")).unwrap(),
            "[[B/Renamed|Keep label]]"
        );
        assert_eq!(
            vault.resolve(Path::new("B/Renamed.md"), "/A/Sibling"),
            Some("A/Sibling.md".into())
        );
    }
    #[test]
    fn folder_actions_reject_collisions_escapes_and_recursive_moves_without_changes() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Note.md"), "note").unwrap();
        fs::write(dir.path().join("Index.md"), "[[Note]]").unwrap();
        let mut vault = Vault::open(dir.path()).unwrap();
        vault.create_folder("Empty").unwrap();
        assert!(vault.folders.contains(&PathBuf::from("Empty")));
        assert!(vault.create_folder("empty").is_err());
        for to in [
            "Index.md",
            "index.md",
            "../Outside.md",
            "/Outside.md",
            "Missing/Note.md",
        ] {
            assert!(
                vault
                    .move_entry(Path::new("Note.md"), Path::new(to))
                    .is_err(),
                "{to}"
            );
        }
        assert!(
            vault
                .move_entry(Path::new("Empty"), Path::new("Empty/Nested"))
                .is_err()
        );
        assert_eq!(vault.read(Path::new("Index.md")).unwrap(), "[[Note]]");
        assert_eq!(vault.read(Path::new("Note.md")).unwrap(), "note");
        fs::create_dir(dir.path().join("External")).unwrap();
        assert!(vault.refresh().unwrap());
        assert!(vault.folders.contains(&PathBuf::from("External")));
        fs::remove_dir(dir.path().join("External")).unwrap();
        assert!(vault.refresh().unwrap());
        assert!(!vault.folders.contains(&PathBuf::from("External")));
    }
    #[cfg(unix)]
    #[test]
    fn moves_and_new_folders_reject_symlink_ancestors() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Note.md"), "note").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("Link")).unwrap();
        let mut vault = Vault::open(dir.path()).unwrap();
        assert!(vault.create_folder("Link/Folder").is_err());
        assert!(
            vault
                .move_entry(Path::new("Note.md"), Path::new("Link/Note.md"))
                .is_err()
        );
        assert!(
            vault
                .move_entry(Path::new("Link"), Path::new("Moved"))
                .is_err()
        );
        assert!(dir.path().join("Note.md").is_file());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }
    #[test]
    fn refresh_reuses_unchanged_notes_and_updates_search_and_links() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("Source.md"), "[[Target]] [[Target]]").unwrap();
        fs::write(temp.path().join("Target.md"), "old body").unwrap();
        let mut vault = Vault::open(temp.path()).unwrap();
        let snapshot = vault.clone();
        assert!(!vault.refresh().unwrap());
        assert!(Arc::ptr_eq(&vault.notes, &snapshot.notes));
        assert_eq!(vault.backlinks(Path::new("Target.md")).len(), 1);

        let target = temp.path().join("Target.md");
        fs::write(&target, "new body with more text").unwrap();
        assert!(vault.refresh().unwrap());
        assert!(Arc::ptr_eq(&vault.notes[0], &snapshot.notes[0]));
        assert!(!Arc::ptr_eq(&vault.notes[1], &snapshot.notes[1]));
        assert!(vault.notes[1].matches_text("new body with more text"));
        assert!(snapshot.notes[1].matches_text("old body"));
        assert!(!snapshot.notes[1].matches_text("new body with more text"));
        assert!(!vault.refresh().unwrap());

        fs::remove_file(temp.path().join("Source.md")).unwrap();
        assert!(vault.refresh().unwrap());
        assert!(vault.backlinks(Path::new("Target.md")).is_empty());
        assert!(vault.search("Source").is_empty());
    }

    #[test]
    fn title_index_tracks_ambiguity_and_local_saves_update_backlinks() {
        let temp = tempfile::tempdir().unwrap();
        let mut vault = Vault::open(temp.path()).unwrap();
        let source = vault.create("Source", "[[Target]]").unwrap();
        let target = vault.create("a/Target", "").unwrap();
        assert_eq!(vault.resolve(&source, "Target"), Some(target.clone()));
        assert_eq!(vault.backlinks(&target).len(), 1);
        let other = vault.create("b/Target", "").unwrap();
        assert_eq!(vault.resolve(&source, "Target"), None);
        assert!(vault.backlinks(&target).is_empty());
        assert_eq!(vault.resolve(&source, "missing/Target"), None);
        vault
            .save(&source, "[[Target]]", "[[a/Target]] [[a/Target]]")
            .unwrap();
        assert_eq!(vault.backlinks(&target).len(), 1);
        fs::remove_file(temp.path().join(other)).unwrap();
        assert!(vault.refresh().unwrap());
        assert_eq!(vault.resolve(&source, "Target"), Some(target));
    }

    #[cfg(unix)]
    #[test]
    fn refresh_detects_same_length_rewrites_with_restored_mtime() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("Note.md");
        fs::write(&path, "before").unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let mut vault = Vault::open(temp.path()).unwrap();
        fs::write(&path, "after!").unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert!(vault.refresh().unwrap());
        assert!(vault.notes[0].matches_text("after!"));
        assert!(!vault.notes[0].matches_text("before"));
    }

    #[test]
    fn path_filter_does_not_search_bodies_and_observes_cancellation() {
        let temp = tempfile::tempdir().unwrap();
        let mut vault = Vault::open(temp.path()).unwrap();
        vault.create("Note", "searchable").unwrap();
        assert!(vault.search_indices("SEARCHABLE").is_empty());
        assert_eq!(vault.search_indices("NOTE"), vec![0]);
        assert!(
            vault
                .search_indices_cancellable("NOTE", &AtomicBool::new(true))
                .is_empty()
        );
    }

    #[test]
    fn create_save_reopen_and_conflict_preserves_disk() {
        let temp = tempfile::tempdir().unwrap();
        let mut v = Vault::open(temp.path()).unwrap();
        let path = v.create("Ideas/Hello", "first").unwrap();
        v.save(&path, "first", "second").unwrap();
        assert_eq!(
            Vault::open(temp.path()).unwrap().read(&path).unwrap(),
            "second"
        );
        fs::write(temp.path().join(&path), "external").unwrap();
        assert!(matches!(
            v.save(&path, "second", "mine"),
            Err(SaveError::Conflict)
        ));
        assert_eq!(v.read(&path).unwrap(), "external");
        assert!(v.create("Ideas/Hello", "overwrite").is_err());
        assert!(v.create("ideas/hello", "case overwrite").is_err());
    }
    #[test]
    fn filenames_are_portable_and_confined() {
        for name in [
            "",
            "../escape",
            "/absolute",
            "a/../../b",
            "a\\b",
            "CON.md",
            "NUL",
            ".hidden",
            "a:",
            "foo.",
        ] {
            assert!(Vault::note_path(name).is_err(), "{name}");
        }
        assert_eq!(
            Vault::note_path("Ideas/İstanbul").unwrap(),
            PathBuf::from("Ideas/İstanbul.md")
        );
    }
    #[test]
    fn links_resolve_relative_and_ambiguous_titles_stay_unresolved() {
        let temp = tempfile::tempdir().unwrap();
        let mut v = Vault::open(temp.path()).unwrap();
        let a = v.create("a/Note", "[[Other#Heading|Read]]").unwrap();
        let b = v.create("a/Other", "").unwrap();
        v.create("b/Other", "").unwrap();
        assert_eq!(v.resolve(&a, "Other#Heading"), Some(b.clone()));
        assert_eq!(v.resolve(Path::new("Root.md"), "Other"), None);
        assert_eq!(v.backlinks(&b)[0].path, a);
        assert!(v.search("heading").is_empty());
        assert_eq!(v.backlinks(&b)[0].links[0].target, "Other#Heading");
    }
    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_escape_vault() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), temp.path().join("link")).unwrap();
        let mut v = Vault::open(temp.path()).unwrap();
        assert!(v.create("link/escape", "no").is_err());
        assert!(!outside.path().join("escape.md").exists());
    }
}
