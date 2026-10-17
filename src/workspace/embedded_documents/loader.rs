//! File reads, complete parsing, default folding and memory estimates run off UI.
use super::*;
use std::{collections::HashSet, fs, io::Read, mem::size_of, sync::LazyLock};

// A dropped GPUI task cannot interrupt a synchronous read already executing.
// The permit stays held until that work returns, including during cancellation.
static SLOTS: LazyLock<Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(MAX_ACTIVE)));

pub(super) async fn load(input: Input, cancelled: Arc<AtomicBool>) -> Loaded {
    let _permit = SLOTS
        .clone()
        .acquire_owned()
        .await
        .map_err(|error| error.to_string())?;
    check(&cancelled)?;
    let source = match (input.token, input.open) {
        (SourceToken::Open { .. }, Some((source, _))) if source.len() > MAX_FILE_BYTES => {
            return Err(too_large());
        }
        (SourceToken::Open { .. }, Some((source, prepared))) => {
            let prepared = prepared.filter(|prepared| prepared.analysis_complete());
            return prepare(input.path, input.token, source, prepared, &cancelled);
        }
        (SourceToken::Saved { .. }, None) => {
            let source = read_source(&input.absolute_path, &cancelled)?;
            Arc::new(if source.contains('\r') {
                opalglass::document::normalize(&source)
            } else {
                source
            })
        }
        _ => return Err("The open embedded note snapshot is unavailable. Retry.".into()),
    };
    prepare(input.path, input.token, source, None, &cancelled)
}

fn prepare(
    path: PathBuf,
    token: SourceToken,
    source: Arc<String>,
    prepared: Option<Arc<Prepared>>,
    cancelled: &AtomicBool,
) -> Loaded {
    check(cancelled)?;
    let prepared = match prepared {
        Some(prepared) => prepared,
        None => Arc::new(
            Prepared::new_cancellable(&source, cancelled)
                .ok_or("Embedded note loading cancelled")?,
        ),
    };
    check(cancelled)?;
    let mut projection = opalglass::callouts::Projection::default();
    projection.rebuild(prepared.rows.len(), &prepared.callouts);
    projection.with_footnotes(prepared.rows.len(), &prepared.footnotes, &prepared.callouts);
    let bytes = estimate(&source, source.capacity(), &prepared);
    check(cancelled)?;
    Ok(Arc::new(Document {
        path,
        source,
        prepared,
        token,
        projection,
        bytes,
    }))
}
fn check(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) {
        Err("Embedded note loading cancelled".into())
    } else {
        Ok(())
    }
}
fn too_large() -> String {
    SOURCE_TOO_LARGE.into()
}
fn read_source(path: &std::path::Path, cancelled: &AtomicBool) -> Result<String, String> {
    for ancestor in path.ancestors() {
        check(cancelled)?;
        if fs::symlink_metadata(ancestor)
            .map_err(|error| error.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("Embedded notes cannot follow symlinks.".into());
        }
    }
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    let mut chunk = [0; 64 * 1024];
    loop {
        check(cancelled)?;
        let read = file.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        if bytes.len().saturating_add(read) > MAX_FILE_BYTES {
            return Err(too_large());
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    String::from_utf8(bytes).map_err(|_| "The embedded note is not valid UTF-8.".into())
}

/// A conservative admission estimate, counting shared source/preparations again
/// even when an open tab also owns them. The source multiplier reserves room for
/// private live metadata, anchor lookup maps, allocator overhead and capacities.
/// Table payloads are counted once across their many bounded display chunks.
fn estimate(source: &str, source_capacity: usize, prepared: &Prepared) -> usize {
    let mut bytes = source_capacity
        .saturating_add(source.len().saturating_mul(3))
        .saturating_add(size_of::<Document>() + size_of::<Prepared>())
        .saturating_add(
            prepared
                .rows
                .capacity()
                .saturating_mul(size_of::<PreviewRow>()),
        );
    let mut tables = HashSet::new();
    for row in &prepared.rows {
        bytes = bytes.saturating_add(row.text.len() + 64);
        bytes = bytes.saturating_add(spans_size(&row.spans, row.spans.capacity()));
        if let Some(code) = &row.code {
            // Code Arcs are shared across chunks; this extra chunk-sized reserve
            // covers the complete code without counting it once per chunk.
            bytes = bytes.saturating_add(code.len().min(1024));
        }
        if let Some(chunk) = &row.table {
            bytes = bytes.saturating_add(128 + chunk.row.cells.len() * 32);
            if tables.insert(Arc::as_ptr(&chunk.row)) {
                for cell in &chunk.row.cells {
                    bytes = bytes
                        .saturating_add(cell.text.len() + 128)
                        .saturating_add(spans_size(&cell.spans, cell.spans.capacity()));
                }
            }
        }
    }
    bytes = bytes.saturating_add(
        prepared
            .embeds
            .capacity()
            .saturating_mul(size_of::<opalglass::embeds::Occurrence>()),
    );
    for occurrence in &prepared.embeds {
        bytes = bytes.saturating_add(occurrence.request.target.len() + 32);
    }
    for link in prepared.links.iter() {
        bytes = bytes.saturating_add(128 + link.target.len() + link.label.len());
    }
    bytes = bytes.saturating_add(prepared.source_syntax.len().saturating_mul(64));
    bytes = bytes.saturating_add(prepared.anchors.blocks.len().saturating_mul(256));
    bytes = bytes.saturating_add(
        prepared
            .anchors
            .markers
            .capacity()
            .saturating_mul(size_of::<std::ops::Range<usize>>()),
    );
    for heading in &prepared.anchors.headings {
        bytes = bytes.saturating_add(256 + heading.title.len() * 4);
    }
    for note in &prepared.footnotes.definitions {
        bytes = bytes.saturating_add(
            256 + note.label.len() + note.sections.len() * 32 + note.references.len() * 16,
        );
    }
    // Full-note footer/callout projection may keep two row-index vectors.
    bytes
        .saturating_add(prepared.footnotes.references.len().saturating_mul(128))
        .saturating_add(prepared.footnotes.definitions.len().saturating_mul(128))
        .saturating_add(prepared.rows.len().saturating_mul(32))
        .saturating_add(prepared.callouts.len().saturating_mul(256))
}
fn spans_size(spans: &[markdown::Span], capacity: usize) -> usize {
    spans.iter().fold(
        capacity.saturating_mul(size_of::<markdown::Span>()),
        |bytes, span| {
            bytes
                .saturating_add(64)
                .saturating_add(span.style.target.as_ref().map_or(0, String::capacity))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{MAX_FILE_BYTES, SourceToken, estimate, load};
    use std::{
        fs,
        sync::{Arc, atomic::AtomicBool},
    };

    #[test]
    fn reads_are_bounded_cancelled_and_utf8_checked() {
        let folder = tempfile::tempdir().unwrap();
        // Match Vault::open: macOS temporary roots can contain a /var symlink.
        let path = folder.path().canonicalize().unwrap().join("A.md");
        for (bytes, expected) in [
            (vec![b'x'; MAX_FILE_BYTES + 1], super::SOURCE_TOO_LARGE),
            (vec![0xff], "The embedded note is not valid UTF-8."),
        ] {
            fs::write(&path, bytes).unwrap();
            let input = super::Input {
                path: "A.md".into(),
                absolute_path: path.clone(),
                token: SourceToken::saved_fixture(1),
                open: None,
            };
            assert_eq!(
                futures::executor::block_on(load(input, Arc::new(AtomicBool::new(false))))
                    .err()
                    .as_deref(),
                Some(expected)
            );
        }
        fs::write(&path, "valid").unwrap();
        let input = super::Input {
            path: "A.md".into(),
            absolute_path: path,
            token: SourceToken::saved_fixture(1),
            open: None,
        };
        assert_eq!(
            futures::executor::block_on(load(input, Arc::new(AtomicBool::new(true))))
                .err()
                .as_deref(),
            Some("Embedded note loading cancelled")
        );
    }
    #[test]
    fn saved_crlf_snapshots_share_editor_normalization_and_anchor_offsets() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().canonicalize().unwrap().join("A.md");
        let raw = "# Heading\r\n\r\nText é[^n] ^block\r\n\r\n[^n]: Footnote\r\n";
        fs::write(&path, raw).unwrap();
        let input = super::Input {
            path: "A.md".into(),
            absolute_path: path.clone(),
            token: SourceToken::saved_fixture(1),
            open: None,
        };
        let document =
            futures::executor::block_on(load(input, Arc::new(AtomicBool::new(false)))).unwrap();
        let buffer = opalglass::buffer::Buffer::new(opalglass::document::normalize(raw));
        assert_eq!(document.source.as_str(), buffer.text());
        let body = buffer.text().find("Text é").unwrap();
        assert_eq!(
            document.prepared.anchors.target("^block").unwrap().start,
            body
        );
        assert_eq!(
            document.prepared.footnotes.references[0].source.start,
            buffer.text().find("[^n]").unwrap()
        );
        assert!(
            document
                .prepared
                .rows
                .iter()
                .all(|row| buffer.text().get(row.source.clone()).is_some())
        );
        assert_eq!(fs::read_to_string(path).unwrap(), raw);
    }

    #[test]
    fn estimates_include_rendering_and_remain_finite_for_shared_table_chunks() {
        let source = format!("| A | B |\n| - | - |\n| {} | last |", "é ".repeat(6000));
        let prepared = opalglass::prepared::Prepared::new(&source);
        let bytes = estimate(&source, source.capacity(), &prepared);
        assert!(bytes > source.len() * 4);
        assert!(bytes < source.len() * 16);
    }
}
