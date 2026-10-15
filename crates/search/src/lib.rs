//! Persistent, rebuildable full-text search. Markdown files remain the source of truth.
pub mod query;
pub mod snippets;
pub use query::{Engine, Hit, Requests, Results, Ticket};
use snippets::FileVersion;
pub use snippets::{Snippet, Snippets};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fs,
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tantivy::{
    DocAddress, Index, IndexReader, IndexWriter, Searcher, TantivyDocument, Term, doc,
    indexer::IndexWriterOptions,
    schema::{Field, STORED, STRING, Schema, TEXT, Value},
};

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
pub const FORMAT: u64 = 3;
const BATCH_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy)]
pub struct Fields {
    path: Field,
    title: Field,
    exact: Field,
    body: Field,
    checkpoints: Field,
    source_len: Field,
    source_mtime: Field,
}
impl Fields {
    fn from(schema: &Schema) -> Result<Self> {
        Ok(Self {
            path: schema.get_field("path")?,
            title: schema.get_field("title")?,
            exact: schema.get_field("exact_title")?,
            body: schema.get_field("body")?,
            checkpoints: schema.get_field("checkpoints")?,
            source_len: schema.get_field("source_len")?,
            source_mtime: schema.get_field("source_mtime")?,
        })
    }
}
fn schema() -> Schema {
    let mut builder = Schema::builder();
    builder.add_text_field("path", STRING | STORED);
    builder.add_text_field("title", TEXT | STORED);
    builder.add_text_field("exact_title", STRING);
    builder.add_text_field("body", TEXT);
    builder.add_bytes_field("checkpoints", STORED);
    builder.add_u64_field("source_len", STORED);
    builder.add_text_field("source_mtime", STORED);
    builder.build()
}
fn text(document: &TantivyDocument, field: Field) -> String {
    document
        .get_first(field)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned()
}
fn check(cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "Search indexing cancelled",
        )
        .into());
    }
    Ok(())
}
pub(crate) fn checked_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    let mut path = root.to_owned();
    for part in relative.components() {
        let Component::Normal(name) = part else {
            return Err("Search path must stay inside the vault".into());
        };
        path.push(name);
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err("Search does not follow symlinks".into());
        }
    }
    if relative.as_os_str().is_empty() {
        return Err("Empty note path".into());
    }
    Ok(path)
}
fn documents(searcher: &Searcher) -> Result<Vec<(DocAddress, TantivyDocument)>> {
    let mut documents = vec![];
    for (ord, segment) in searcher.segment_readers().iter().enumerate() {
        for doc in 0..segment.max_doc() {
            if segment.is_deleted(doc) {
                continue;
            }
            let address = DocAddress::new(ord as u32, doc);
            documents.push((address, searcher.doc::<TantivyDocument>(address)?));
        }
    }
    Ok(documents)
}
pub(crate) fn stored_paths(searcher: &Searcher, f: Fields) -> Result<Vec<(String, DocAddress)>> {
    let mut paths: Vec<_> = documents(searcher)?
        .into_iter()
        .map(|(a, d)| (text(&d, f.path).to_lowercase(), a))
        .collect();
    paths.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(paths)
}
fn versions(searcher: &Searcher, f: Fields) -> Result<HashMap<PathBuf, FileVersion>> {
    documents(searcher)?
        .into_iter()
        .map(|(_, d)| {
            if d.get_first(f.body).is_some() {
                return Err("Search cache unexpectedly contains stored bodies".into());
            }
            Ok((
                PathBuf::from(text(&d, f.path)),
                FileVersion {
                    len: d
                        .get_first(f.source_len)
                        .and_then(|v| v.as_u64())
                        .ok_or("Missing indexed version")?,
                    modified: text(&d, f.source_mtime),
                },
            ))
        })
        .collect()
}
struct Worker {
    writer: IndexWriter,
    reader: IndexReader,
    versions: HashMap<PathBuf, FileVersion>,
}
pub struct SearchIndex {
    root: PathBuf,
    index: Index,
    fields: Fields,
    worker: Mutex<Worker>,
    storage: Option<Arc<tempfile::TempDir>>,
    pub directory: PathBuf,
    pub persistent: bool,
}
#[derive(Debug, Default)]
pub struct SyncReport {
    pub updated: usize,
    pub removed: usize,
    pub notes: usize,
}
impl SearchIndex {
    /// cache_base=None creates an isolated temporary index (tests / unavailable cache).
    /// A locked, incompatible or damaged cache falls back to a session-local index.
    pub fn open(root: &Path, cache_base: Option<&Path>) -> Result<Self> {
        let root = root.canonicalize()?;
        if let Some(base) = cache_base {
            let hash = root
                .as_os_str()
                .as_encoded_bytes()
                .iter()
                .fold(0xcbf29ce484222325u64, |h, b| {
                    (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
                });
            let directory = base.join(format!("v{FORMAT}-{hash:016x}"));
            // Never write caches into a vault, even if XDG_CACHE_HOME was customized.
            if base.is_absolute()
                && !base.starts_with(&root)
                && base
                    .ancestors()
                    .find(|p| p.exists())
                    .and_then(|p| p.canonicalize().ok())
                    .is_some_and(|p| !p.starts_with(&root))
                && fs::create_dir_all(base).is_ok()
                && !base.canonicalize()?.starts_with(&root)
                && let Ok(index) = Self::open_at(root.clone(), directory, None)
            {
                return Ok(index);
            }
        }
        let storage = Arc::new(tempfile::tempdir()?);
        Self::open_at(root, storage.path().to_owned(), Some(storage))
    }
    fn open_at(
        root: PathBuf,
        directory: PathBuf,
        storage: Option<Arc<tempfile::TempDir>>,
    ) -> Result<Self> {
        fs::create_dir_all(&directory)?;
        let manifest = directory.join("source.json");
        let exists = directory.join("meta.json").exists();
        let index = if exists {
            let source: serde_json::Value = serde_json::from_slice(&fs::read(&manifest)?)?;
            if source["format"] != FORMAT || source["root"].as_str() != root.to_str() {
                return Err("Incompatible search cache".into());
            }
            Index::open_in_dir(&directory)?
        } else {
            Index::create_in_dir(&directory, schema())?
        };
        let fields = Fields::from(&index.schema())?;
        let writer = index.writer_with_options(
            IndexWriterOptions::builder()
                .num_worker_threads(1)
                .num_merge_threads(1)
                .memory_budget_per_thread(32 * 1024 * 1024)
                .build(),
        )?;
        let reader = index
            .reader_builder()
            .reload_policy(tantivy::ReloadPolicy::Manual)
            .try_into()?;
        let known = versions(&reader.searcher(), fields)?;
        if !exists {
            fs::write(
                manifest,
                serde_json::to_vec(&serde_json::json!({"root": root, "format": FORMAT}))?,
            )?;
        }
        Ok(Self {
            root,
            index,
            fields,
            directory,
            persistent: storage.is_none(),
            storage,
            worker: Mutex::new(Worker {
                writer,
                reader,
                versions: known,
            }),
        })
    }
    /// Call on a worker. Paths come from a completed vault inventory, never from query text.
    /// Unchanged notes are checked by metadata and are not read/tokenized again.
    pub fn synchronize(&self, paths: &[PathBuf], cancelled: &AtomicBool) -> Result<SyncReport> {
        check(cancelled)?;
        let mut worker = self.worker.lock().map_err(|_| "Search writer failed")?;
        check(cancelled)?;
        let result = self.synchronize_locked(&mut worker, paths, cancelled);
        if result.is_err() {
            // Keep previous committed batches. Reconstruct versions after rolling back
            // the unfinished batch, so a cancelled update can safely resume.
            let _ = worker.writer.rollback();
            worker.reader.reload()?;
            worker.versions = versions(&worker.reader.searcher(), self.fields)?;
        }
        result
    }
    fn synchronize_locked(
        &self,
        worker: &mut Worker,
        paths: &[PathBuf],
        cancelled: &AtomicBool,
    ) -> Result<SyncReport> {
        let mut report = SyncReport::default();
        let wanted: HashSet<_> = paths.iter().cloned().collect();
        let removed: Vec<_> = worker
            .versions
            .keys()
            .filter(|p| !wanted.contains(*p))
            .cloned()
            .collect();
        for path in removed {
            check(cancelled)?;
            worker.writer.delete_term(Term::from_field_text(
                self.fields.path,
                &path.to_string_lossy(),
            ));
            worker.versions.remove(&path);
            report.removed += 1;
        }
        let mut tokenizer = self.index.tokenizer_for_field(self.fields.body)?;
        let mut pending_bytes = 0;
        for relative in paths {
            check(cancelled)?;
            let path = checked_path(&self.root, relative)?;
            let before = FileVersion::read(&path)?;
            if worker.versions.get(relative) == Some(&before) {
                continue;
            }
            let body = fs::read_to_string(&path)?;
            check(cancelled)?;
            if before != FileVersion::read(&path)? || before.len != body.len() as u64 {
                return Err(
                    "A note changed during search indexing; retrying on the next refresh".into(),
                );
            }
            let checkpoints = snippets::checkpoints(&mut tokenizer, &body);
            check(cancelled)?;
            let title = relative
                .file_stem()
                .ok_or("Missing note title")?
                .to_string_lossy();
            let path_text = relative.to_string_lossy();
            worker
                .writer
                .delete_term(Term::from_field_text(self.fields.path, &path_text));
            worker.writer.add_document(doc!(self.fields.path => path_text.as_ref(), self.fields.title => title.as_ref(),
                self.fields.exact => title.to_lowercase(), self.fields.body => body.as_str(), self.fields.checkpoints => checkpoints,
                self.fields.source_len => before.len, self.fields.source_mtime => before.modified.as_str()))?;
            pending_bytes += body.len();
            worker.versions.insert(relative.clone(), before);
            report.updated += 1;
            if pending_bytes >= BATCH_BYTES {
                worker.writer.commit()?;
                pending_bytes = 0;
            }
        }
        check(cancelled)?;
        if report.updated + report.removed > 0 {
            worker.writer.commit()?;
            worker.reader.reload()?;
        }
        report.notes = worker.versions.len();
        Ok(report)
    }
    /// Immutable searcher snapshots keep UI queries independent of the writer lock.
    /// Construct on a worker after synchronization, then share through Arc.
    pub fn snapshot(&self) -> Result<Engine> {
        let worker = self.worker.lock().map_err(|_| "Search writer failed")?;
        let searcher = worker.reader.searcher();
        let paths = Arc::new(stored_paths(&searcher, self.fields)?);
        Ok(Engine {
            index: self.index.clone(),
            searcher,
            fields: self.fields,
            root: self.root.clone(),
            paths,
            _storage: self.storage.clone(),
        })
    }
}
/// Platform-neutral cache selection; no CPU-specific or OS-specific search engine code.
pub fn cache_directory() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|p| p.join("Opalglass/Search"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|p| p.join("Library/Caches/Opalglass/Search"))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .map(|p| p.join("opalglass/search"))
    }
}

#[cfg(test)]
mod tests;
