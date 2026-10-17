use super::*;

#[derive(Debug, Clone)]
pub struct Note {
    pub path: PathBuf,
    pub title: String,
    pub links: Vec<WikiLink>,
    pub tags: Vec<String>,
    pub anchors: Option<crate::anchors::Index>,
    pub references: Vec<crate::links::Reference>,
    pub(super) content_hash: [u8; 32],
    pub(super) search_path: String,
    pub(super) stamp: Option<Stamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    #[cfg(unix)]
    changed: (i64, i64),
}

impl Stamp {
    pub(super) fn new(meta: &fs::Metadata) -> Self {
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
    pub(super) fn discovered(path: PathBuf) -> Self {
        let mut note = Self::new(path, "", None);
        note.anchors = None;
        note
    }
    /// A precomputed saved-content identity. Discovery has no body fingerprint
    /// yet; indexing supplies it without retaining another copy of the text.
    pub fn content_fingerprint(&self) -> Option<[u8; 32]> {
        self.anchors.as_ref().map(|_| self.content_hash)
    }
    /// Compare saved contents without keeping another copy of the note body.
    /// Writes still compare the full disk text immediately before replacement.
    pub fn matches_text(&self, text: &str) -> bool {
        self.content_hash == <[u8; 32]>::from(Sha256::digest(text.as_bytes()))
    }
    pub fn has_tag(&self, query: &str) -> bool {
        let query = query.trim_start_matches('#').to_lowercase();
        self.tags.iter().any(|tag| {
            let tag = tag.to_lowercase();
            tag == query
                || tag
                    .strip_prefix(&query)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    }
    pub fn modified(&self) -> Option<SystemTime> {
        self.stamp.as_ref().and_then(|stamp| stamp.modified)
    }
    pub fn created(&self) -> Option<SystemTime> {
        self.stamp.as_ref().and_then(|stamp| stamp.created)
    }
    pub(super) fn new(path: PathBuf, text: &str, stamp: Option<Stamp>) -> Self {
        let analysis = crate::links::analyze(text);
        Self {
            title: path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            search_path: path.to_string_lossy().to_lowercase(),
            links: analysis.wiki_links,
            tags: analysis.tags,
            references: analysis.references,
            anchors: Some(analysis.anchors),
            path,
            content_hash: Sha256::digest(text.as_bytes()).into(),
            stamp,
        }
    }
}

impl Vault {
    /// Metadata lookup through the existing path index; no filesystem work.
    pub fn note(&self, path: &Path) -> Option<&Note> {
        self.indexes
            .paths
            .get(path)
            .map(|&index| self.notes[index].as_ref())
    }
}
