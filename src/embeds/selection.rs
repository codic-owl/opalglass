use super::{Key, Target};
use crate::prepared::Prepared;
use std::{ops::Range, path::PathBuf, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectError {
    NotReady,
    MissingFragment,
    InvalidSourceLength,
}

/// A view onto a complete preparation. Rows, links, reference definitions,
/// callouts and footnotes retain their full-document parsing context.
#[derive(Clone, Debug)]
pub struct Selection {
    pub path: PathBuf,
    pub prepared: Arc<Prepared>,
    pub source: Range<usize>,
    /// Primary rows intersecting the selection, including synthetic footer
    /// anchors inside this interval. Renderers apply the document's footnote
    /// projection and can obtain referenced definitions from complete context.
    pub rows: Range<usize>,
}

impl Target {
    pub fn select(
        self,
        prepared: Arc<Prepared>,
        source_len: usize,
    ) -> Result<Selection, SelectError> {
        if !prepared.complete {
            return Err(SelectError::NotReady);
        }
        if prepared
            .rows
            .last()
            .is_some_and(|row| row.source.end > source_len)
        {
            return Err(SelectError::InvalidSourceLength);
        }
        let fragment = self.fragment.as_deref().unwrap_or_default();
        let source = if fragment.trim().is_empty() {
            0..source_len
        } else {
            prepared
                .anchors
                .target(fragment)
                .ok_or(SelectError::MissingFragment)?
        };
        if source.end > source_len {
            return Err(SelectError::InvalidSourceLength);
        }
        let rows = if fragment.trim().is_empty() {
            0..prepared.rows.len()
        } else {
            let start = prepared
                .rows
                .partition_point(|row| row.source.end <= source.start);
            let end = prepared
                .rows
                .partition_point(|row| row.source.start < source.end);
            start.min(end)..end
        };
        Ok(Selection {
            path: self.path,
            prepared,
            source,
            rows,
        })
    }
}

impl Selection {
    pub fn key(&self) -> Key {
        Key {
            path: self.path.clone(),
            source: self.source.clone(),
        }
    }
}
