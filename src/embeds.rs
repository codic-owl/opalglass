//! CPU-only note transclusion metadata. Callers load and cache documents on demand.
use crate::{markdown::Span, prepared::PreviewRow, vault::Vault};
use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

mod selection;
pub use selection::{SelectError, Selection};
mod occurrences;
pub use occurrences::{Occurrence, occurrences};

/// One image/embed occurrence. Formatted labels may have several inline spans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub range: Range<usize>,
    pub target: Arc<str>,
    /// Parser identity, not an original-source byte offset. It also lets callers
    /// deduplicate labels split across bounded preview rows.
    pub image_start: usize,
    pub part: usize,
}

pub fn requests(row: &PreviewRow) -> Vec<Request> {
    group_spans(&row.spans)
}

/// Also accepts the clipped spans of a table cell; ranges remain cell-local.
pub fn group_spans(spans: &[Span]) -> Vec<Request> {
    let mut requests: Vec<Request> = Vec::new();
    for (part, span) in spans.iter().enumerate() {
        let Some(target) = span.style.target.as_deref().filter(|_| span.style.image) else {
            continue;
        };
        if let Some(previous) = requests.last_mut()
            && previous.range.end == span.range.start
            && previous.image_start == span.style.image_start
            && previous.target.as_ref() == target
        {
            previous.range.end = span.range.end;
        } else {
            requests.push(Request {
                range: span.range.clone(),
                target: Arc::from(target),
                image_start: span.style.image_start,
                part,
            });
        }
    }
    requests
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// Vault-relative, resolved using the note containing this embed.
    pub path: PathBuf,
    pub fragment: Option<Arc<str>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolveError {
    MissingNote,
}

/// Metadata-only resolution: no file probing, parsing, or note body reads.
/// `None` leaves images, external URLs and other attachments to their renderer.
pub fn resolve(vault: &Vault, origin: &Path, target: &str) -> Result<Option<Target>, ResolveError> {
    let path = target.split(['#', '?']).next().unwrap_or(target).trim();
    let decoded = crate::links::decode_url(path).unwrap_or_else(|| path.to_owned());
    if decoded.contains(':')
        || decoded.starts_with("//")
        || crate::attachments::is_image(Path::new(&decoded))
        || vault.resolve_attachment(origin, target).is_some()
    {
        return Ok(None);
    }
    let path = vault
        .resolve(origin, target)
        .or_else(|| vault.resolve_url(origin, target))
        .filter(|path| vault.contains_path(path))
        .ok_or(ResolveError::MissingNote)?;
    Ok(Some(Target {
        path,
        fragment: target
            .split_once('#')
            .map(|(_, fragment)| Arc::from(fragment)),
    }))
}

/// Identity is the selected source, so separate sections of one note may nest.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub path: PathBuf,
    pub source: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub depth: usize,
    pub instances: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            depth: 8,
            instances: 64,
        }
    }
}

/// Share one budget across sibling branches of a document expansion. Failed
/// entries consume nothing; successful occurrences count even if repeated.
#[derive(Debug)]
pub struct Budget {
    limits: Limits,
    used: usize,
}
impl Budget {
    pub fn new(limits: Limits) -> Self {
        Self { limits, used: 0 }
    }
    pub fn used(&self) -> usize {
        self.used
    }
    pub fn remaining(&self) -> usize {
        self.limits.instances.saturating_sub(self.used)
    }
}
impl Default for Budget {
    fn default() -> Self {
        Self::new(Limits::default())
    }
}

#[derive(Clone, Debug)]
pub struct Ancestry {
    keys: Vec<Key>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    Cycle,
    Depth,
    Instances,
}
impl Ancestry {
    /// The host note is an ancestor, but does not consume an embed instance.
    pub fn root(key: Key) -> Self {
        Self { keys: vec![key] }
    }
    pub fn enter(&self, key: Key, budget: &mut Budget) -> Result<Self, Stop> {
        if self.keys.contains(&key) {
            return Err(Stop::Cycle);
        }
        if self.keys.len().saturating_sub(1) >= budget.limits.depth {
            return Err(Stop::Depth);
        }
        if budget.remaining() == 0 {
            return Err(Stop::Instances);
        }
        budget.used += 1;
        let mut keys = self.keys.clone();
        keys.push(key);
        Ok(Self { keys })
    }
}

#[cfg(test)]
mod tests;
