use super::*;

#[cfg(unix)]
#[test]
fn snippet_rejects_a_note_replaced_with_an_outside_symlink() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.note("Original", "unmistakable")?;
    let engine = fixture.engine()?;
    let requests = Requests::default();
    let ticket = requests.begin();
    let result = engine.search("unmistakable", &ticket)?;
    let outside = fixture.0.join("outside.md");
    fs::write(&outside, "outside the vault")?;
    let original = fixture.0.join("vault/Original.md");
    fs::remove_file(&original)?;
    std::os::unix::fs::symlink(&outside, &original)?;
    assert!(
        Snippets::default()
            .get(&engine, &result.hits[0], &ticket)
            .is_err()
    );
    Ok(())
}

#[test]
fn persistent_incremental_updates_deletes_renames_and_snapshot_isolation() -> Result<()> {
    use std::sync::atomic::AtomicBool;
    let fixture = Fixture::new()?;
    fixture.note("Old", "quasar")?;
    let paths = vec!["Old.md".into()];
    let cache = fixture.0.join("cache");
    let root = fixture.0.join("vault");
    let cancel = AtomicBool::new(false);
    let index = SearchIndex::open(&root, Some(&cache))?;
    assert!(index.persistent);
    assert_eq!(index.synchronize(&paths, &cancel)?.updated, 1);
    let old_snapshot = index.snapshot()?;
    let ticket = Requests::default().begin();
    assert_eq!(old_snapshot.search("\"quasar\"", &ticket)?.hits.len(), 1);
    assert_eq!(index.synchronize(&paths, &cancel)?.updated, 0);
    drop(index);
    let index = SearchIndex::open(&root, Some(&cache))?;
    assert!(index.persistent);
    assert_eq!(index.synchronize(&paths, &cancel)?.updated, 0);
    fixture.note("Old", "supernova")?;
    assert_eq!(index.synchronize(&paths, &cancel)?.updated, 1);
    let current = index.snapshot()?;
    assert!(current.search("\"quasar\"", &ticket)?.hits.is_empty());
    assert_eq!(current.search("\"supernova\"", &ticket)?.hits.len(), 1);
    assert_eq!(old_snapshot.search("\"quasar\"", &ticket)?.hits.len(), 1);
    fs::rename(root.join("Old.md"), root.join("Renamed.md"))?;
    let report = index.synchronize(&["Renamed.md".into()], &cancel)?;
    assert_eq!((report.updated, report.removed), (1, 1));
    let current = index.snapshot()?;
    assert_eq!(
        current.search("\"supernova\"", &ticket)?.hits[0].path,
        "Renamed.md"
    );
    assert!(current.search("\"Old\"", &ticket)?.hits.is_empty());
    fs::remove_file(root.join("Renamed.md"))?;
    assert_eq!(index.synchronize(&[], &cancel)?.removed, 1);
    assert!(
        index
            .snapshot()?
            .search("\"supernova\"", &ticket)?
            .hits
            .is_empty()
    );
    Ok(())
}

#[test]
fn failed_or_cancelled_sync_retries_without_duplicates() -> Result<()> {
    use std::sync::atomic::AtomicBool;
    let fixture = Fixture::new()?;
    fixture.note("A", "before")?;
    fixture.note("B", "before")?;
    let index = SearchIndex::open(&fixture.0.join("vault"), None)?;
    let paths = vec!["A.md".into(), "B.md".into()];
    index.synchronize(&paths, &AtomicBool::new(false))?;
    fixture.note("A", "after")?;
    fs::write(fixture.0.join("vault/B.md"), [0xff])?;
    assert!(index.synchronize(&paths, &AtomicBool::new(false)).is_err());
    fixture.note("B", "after")?;
    assert!(index.synchronize(&paths, &AtomicBool::new(true)).is_err());
    assert_eq!(
        index.synchronize(&paths, &AtomicBool::new(false))?.updated,
        2
    );
    let engine = index.snapshot()?;
    let result = engine.search("\"after\"", &Requests::default().begin())?;
    assert_eq!(result.hits.len(), 2);
    assert_eq!(engine.searcher.num_docs(), 2);
    Ok(())
}

#[test]
fn cache_conflicts_are_isolated_and_cache_never_written_inside_vault() -> Result<()> {
    let fixture = Fixture::new()?;
    let root = fixture.0.join("vault");
    let nested = root.join("do-not-create/cache");
    let index = SearchIndex::open(&root, Some(&nested))?;
    assert!(!index.persistent);
    assert!(!nested.exists());
    let first = SearchIndex::open(&root, Some(&fixture.0.join("cache")))?;
    let second = SearchIndex::open(&root, Some(&fixture.0.join("cache")))?;
    assert!(first.persistent);
    assert!(!second.persistent);
    assert_ne!(first.directory, second.directory);
    Ok(())
}

#[test]
fn synchronization_releases_writer_but_keeps_cache_reserved() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.note("A", "A quasar shines.")?;
    let root = fixture.0.join("vault");
    let cache = fixture.0.join("cache");
    let index = SearchIndex::open(&root, Some(&cache))?;
    let paths = [PathBuf::from("A.md")];
    for expected_updates in [1, 0] {
        assert_eq!(
            index
                .synchronize(&paths, &std::sync::atomic::AtomicBool::new(false))?
                .updated,
            expected_updates
        );
        // A retained Tantivy writer would hold this lock and fail the probe.
        let probe = index
            .index
            .writer_with_options::<tantivy::TantivyDocument>(
                tantivy::indexer::IndexWriterOptions::builder()
                    .num_worker_threads(1)
                    .memory_budget_per_thread(16 * 1024 * 1024)
                    .build(),
            )?;
        probe.wait_merging_threads()?;
        assert!(!SearchIndex::open(&root, Some(&cache))?.persistent);
    }
    let old = index.snapshot()?;
    fixture.note("A", "A nebula glows.")?;
    index.synchronize(&paths, &std::sync::atomic::AtomicBool::new(false))?;
    let ticket = Requests::default().begin();
    assert_eq!(old.search("quasar", &ticket)?.hits.len(), 1);
    assert_eq!(index.snapshot()?.search("nebula", &ticket)?.hits.len(), 1);
    drop(index);
    assert!(SearchIndex::open(&root, Some(&cache))?.persistent);
    Ok(())
}

#[cfg(unix)]
#[test]
fn same_length_edits_with_restored_mtime_invalidate_snippets_and_index() -> Result<()> {
    use std::{os::unix::fs::MetadataExt, time::Instant};

    let fixture = Fixture::new()?;
    fixture.note("Note", "alpha")?;
    let root = fixture.0.join("vault");
    let index = SearchIndex::open(&root, None)?;
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    index.synchronize(&["Note.md".into()], &cancelled)?;
    let engine = index.snapshot()?;
    let ticket = Requests::default().begin();
    let result = engine.search("alpha", &ticket)?;
    let mut snippets = Snippets::default();
    assert_eq!(
        snippets.get(&engine, &result.hits[0], &ticket)?.status,
        "matched"
    );
    let path = root.join("Note.md");
    let original = fs::metadata(&path)?;
    let time = original.modified()?;
    let changed = |metadata: &fs::Metadata| (metadata.ctime(), metadata.ctime_nsec());
    let deadline = Instant::now() + std::time::Duration::from_secs(3);
    loop {
        fs::write(&path, "omega")?;
        fs::File::options()
            .write(true)
            .open(&path)?
            .set_modified(time)?;
        let rewritten = fs::metadata(&path)?;
        assert_eq!(rewritten.len(), original.len());
        assert_eq!(rewritten.modified()?, time);
        if changed(&rewritten) != changed(&original) {
            break;
        }
        // Establish the changed-ctime precondition even when several writes
        // share one filesystem clock tick on a fast runner.
        assert!(
            Instant::now() < deadline,
            "filesystem ctime did not advance"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(
        snippets.get(&engine, &result.hits[0], &ticket)?.status,
        "stale"
    );
    assert_eq!(
        index.synchronize(&["Note.md".into()], &cancelled)?.updated,
        1
    );
    assert!(
        index
            .snapshot()?
            .search("\"alpha\"", &ticket)?
            .hits
            .is_empty()
    );
    Ok(())
}
