use crate::{
    Result, SearchIndex,
    query::{Engine, Requests},
    snippets::{Snippets, WINDOW_BYTES},
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
mod filesystem;

struct Fixture(PathBuf);

#[test]
fn indexing_workers_leave_cpu_capacity_and_share_a_bounded_budget() {
    for (cpus, expected) in [(1, 1), (2, 1), (3, 1), (4, 2), (14, 2), (128, 2)] {
        let workers = crate::indexing_worker_count(cpus);
        assert_eq!(workers, expected);
        // Tantivy requires at least 15,000,000 bytes per worker.
        assert!(crate::WRITER_MEMORY_BYTES / workers >= 15_000_000);
        assert_eq!(crate::WRITER_MEMORY_BYTES, 32 * 1024 * 1024);
    }
}

#[test]
fn cancellation_with_queued_documents_rolls_back_and_retries_without_duplicates() -> Result<()> {
    use crate::SyncProgress;
    use std::sync::atomic::{AtomicBool, Ordering};
    let fixture = Fixture::new()?;
    let mut paths = vec![];
    for i in 0..48 {
        let title = format!("Note {i}");
        fixture.note(&title, &"quasar nebula constellation ".repeat(1024))?;
        paths.push(PathBuf::from(format!("{title}.md")));
    }
    let index = SearchIndex::open(&fixture.0.join("vault"), None)?;
    let old = index.snapshot()?;
    let cancelled = AtomicBool::new(false);
    assert!(
        index
            .synchronize_with_progress(&paths, &cancelled, |p| {
                if matches!(p, SyncProgress::Notes { completed: 32, .. }) {
                    cancelled.store(true, Ordering::Relaxed);
                }
            })
            .is_err()
    );
    assert_eq!(index.snapshot()?.searcher.num_docs(), 0);
    cancelled.store(false, Ordering::Relaxed);
    let report = index.synchronize(&paths, &cancelled)?;
    assert_eq!(report.updated, paths.len());
    let engine = index.snapshot()?;
    assert_eq!(engine.searcher.num_docs(), paths.len() as u64);
    let ticket = Requests::default().begin();
    assert_eq!(
        engine.search("\"quasar\"", &ticket)?.hits.len(),
        paths.len()
    );
    assert!(old.search("\"quasar\"", &ticket)?.hits.is_empty());
    assert_eq!(index.synchronize(&paths, &cancelled)?.updated, 0);
    Ok(())
}

#[test]
fn progress_counts_processed_notes_and_never_completes_failed_updates() -> Result<()> {
    use crate::SyncProgress;
    use std::sync::atomic::{AtomicBool, Ordering};
    let fixture = Fixture::new()?;
    fixture.note("A", "one")?;
    fixture.note("B", "two")?;
    let paths = [PathBuf::from("A.md"), PathBuf::from("B.md")];
    let index = SearchIndex::open(&fixture.0.join("vault"), None)?;
    for updated in [2, 0] {
        let mut events = Vec::new();
        assert_eq!(
            index
                .synchronize_with_progress(&paths, &AtomicBool::new(false), |p| events.push(p))?
                .updated,
            updated
        );
        assert_eq!(
            events,
            vec![
                SyncProgress::Notes {
                    completed: 0,
                    total: 2
                },
                SyncProgress::Notes {
                    completed: 1,
                    total: 2
                },
                SyncProgress::Notes {
                    completed: 2,
                    total: 2
                },
                SyncProgress::Finalizing,
                SyncProgress::Complete,
            ]
        );
    }
    let cancelled = AtomicBool::new(false);
    let mut events = Vec::new();
    assert!(
        index
            .synchronize_with_progress(&paths, &cancelled, |p| {
                events.push(p);
                if matches!(p, SyncProgress::Notes { completed: 1, .. }) {
                    cancelled.store(true, Ordering::Relaxed);
                }
            })
            .is_err()
    );
    assert!(!events.contains(&SyncProgress::Complete));
    fs::remove_file(fixture.0.join("vault/B.md"))?;
    events.clear();
    assert!(
        index
            .synchronize_with_progress(&paths, &AtomicBool::new(false), |p| events.push(p))
            .is_err()
    );
    assert!(!events.contains(&SyncProgress::Complete));
    events.clear();
    index.synchronize_with_progress(&[], &AtomicBool::new(false), |p| events.push(p))?;
    assert_eq!(events.last(), Some(&SyncProgress::Complete));
    Ok(())
}
impl Fixture {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "opalglass-tantivy-test-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir(&path)?;
        fs::create_dir(path.join("vault"))?;
        Ok(Self(path))
    }
    fn note(&self, title: &str, body: &str) -> Result<()> {
        fs::write(self.0.join("vault").join(format!("{title}.md")), body)?;
        Ok(())
    }
    fn engine(&self) -> Result<Engine> {
        let index = SearchIndex::open(&self.0.join("vault"), Some(&self.0.join("cache")))?;
        let paths = fs::read_dir(self.0.join("vault"))?
            .map(|e| e.map(|e| PathBuf::from(e.file_name())))
            .collect::<std::io::Result<Vec<_>>>()?;
        index.synchronize(&paths, &std::sync::atomic::AtomicBool::new(false))?;
        index.snapshot()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn fuzzy_ranking_highlighting_and_query_boundaries() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.note("Glossary", "Glossary is a collection of definitions.")?;
    fixture.note("Glossry", "An exact title must outrank a correction.")?;
    fixture.note("Compassion", "Compassion is caring.")?;
    fixture.note("Hydriotaphia", "Urn burial.")?;
    fixture.note("Body", "Serendipity illuminates divine light. Café 東京.")?;
    fixture.note("Reversed", "light divine")?;
    fixture.note("Popular Tales", "owlglass profile missing term")?;
    fixture.note("Rare Title", "This body has no query terms.")?;
    let engine = fixture.engine()?;
    let requests = Requests::default();
    let ticket = requests.begin();
    let mut snippets = Snippets::default();
    for query in ["glosssary", "glosasry", "GLOSSARY"] {
        let result = engine.search(query, &ticket)?;
        assert_eq!(result.hits[0].title, "Glossary", "{query}");
        let snippet = snippets.get(&engine, &result.hits[0], &ticket)?;
        assert_eq!(snippet.status, "matched");
        assert!(
            snippet
                .highlights
                .iter()
                .any(|r| &snippet.fragment[r.clone()] == "Glossary")
        );
    }
    let result = engine.search("glossry", &ticket)?;
    assert_eq!(result.hits[0].title, "Glossry");
    assert_eq!(result.hits[0].tier, "exact_title");
    assert_eq!(result.hits.len(), 1); // A complete exact title needs no correction.
    let result = engine.search("serendipty", &ticket)?;
    assert_eq!(result.hits[0].tier, "fuzzy_body");
    let snippet = snippets.get(&engine, &result.hits[0], &ticket)?;
    assert!(
        snippet
            .highlights
            .iter()
            .any(|r| &snippet.fragment[r.clone()] == "Serendipity")
    );
    assert_eq!(
        engine.search("hydriataphya", &ticket)?.hits[0].title,
        "Hydriotaphia"
    );
    assert_eq!(
        engine.search("compasion", &ticket)?.hits[0].title,
        "Compassion"
    );
    assert!(engine.search("\"divnie light\"", &ticket)?.hits.is_empty());
    assert!(engine.search("xy", &ticket)?.hits.is_empty());
    assert!(
        engine
            .search("opalglass-profile-missing-term", &ticket)?
            .hits
            .is_empty()
    );
    assert!(engine.search("", &ticket)?.hits.is_empty());
    assert!(engine.search("\"\"", &ticket)?.hits.is_empty());
    assert!(
        engine
            .search("gloss*", &ticket)?
            .hits
            .iter()
            .any(|h| h.title == "Glossary")
    );
    assert!(engine.search("g*", &ticket).is_err());
    assert!(
        engine
            .search("one two three four five six seven eight nine", &ticket)
            .is_err()
    );
    assert!(engine.search("one \"two\"", &ticket).is_err());
    let result = engine.search("\"divine light\"", &ticket)?;
    assert_eq!(result.hits.len(), 1);
    let snippet = snippets.get(&engine, &result.hits[0], &ticket)?;
    assert!(
        snippet
            .highlights
            .iter()
            .any(|r| &snippet.fragment[r.clone()] == "divine light")
    );
    let result = engine.search("divine light", &ticket)?;
    assert_eq!(result.exact_count, 2); // Ordinary terms use AND, irrespective of order.
    let result = engine.search("cafè", &ticket)?;
    let snippet = snippets.get(&engine, &result.hits[0], &ticket)?;
    assert!(
        snippet
            .highlights
            .iter()
            .any(|r| &snippet.fragment[r.clone()] == "Café")
    );
    let result = engine.search("\"Rare Title\"", &ticket)?;
    assert_eq!(
        snippets.get(&engine, &result.hits[0], &ticket)?.status,
        "title_only"
    );
    assert!(
        result.hits[0]
            .document
            .get_first(engine.fields.body)
            .is_none()
    );
    Ok(())
}

#[test]
fn title_prefixes_substrings_and_typos_outrank_body_words() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.note("Iliad", &"Iliad ".repeat(100))?;
    fixture.note("Ailiad commentary", "An embedded filename match.")?;
    fixture.note("Ilja", "A spelling alternative.")?;
    fixture.note(
        "Long account",
        &format!("{} Ilia arrived.", "context ".repeat(10_000)),
    )?;
    let engine = fixture.engine()?;
    let ticket = Requests::default().begin();
    let mut snippets = Snippets::default();
    for query in ["Ilia", "ILIA", "ilia*"] {
        let result = engine.search(query, &ticket)?;
        assert_eq!(result.hits[0].title, "Iliad", "{query}");
        assert_eq!(result.hits[0].tier, "title_prefix");
        let body_index = result
            .hits
            .iter()
            .position(|h| h.title == "Long account")
            .unwrap();
        assert_eq!(result.hits[body_index].tier, "exact");
        if !query.ends_with('*') {
            for title in ["Ailiad commentary", "Ilja"] {
                assert!(result.hits.iter().position(|h| h.title == title).unwrap() < body_index);
            }
        }
        let snippet = snippets.get(&engine, &result.hits[body_index], &ticket)?;
        assert!(
            snippet
                .highlights
                .iter()
                .any(|r| &snippet.fragment[r.clone()] == "Ilia")
        );
    }
    let quoted = engine.search("\"Ilia\"", &ticket)?;
    assert_eq!(quoted.hits.len(), 1);
    assert_eq!(quoted.hits[0].title, "Long account");
    Ok(())
}

#[test]
fn partial_words_match_word_starts_before_embedded_filename_matches() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.note("A Trimorphic Protennoia", "Trimorphic teachings.")?;
    fixture.note("Orphic Hymns", "Ancient Orphic hymns.")?;
    fixture.note("Other traditions", "Ancient Orphism and its history.")?;
    let engine = fixture.engine()?;
    let ticket = Requests::default().begin();
    let mut snippets = Snippets::default();
    for query in ["orphi", "ORPHI"] {
        let result = engine.search(query, &ticket)?;
        assert_eq!(result.hits[0].title, "Orphic Hymns");
        assert_eq!(result.hits[0].tier, "title_prefix");
        assert_eq!(result.hits[1].title, "A Trimorphic Protennoia");
        assert_eq!(result.hits[1].tier, "title_substring");
        assert_eq!(result.hits[2].title, "Other traditions");
        let snippet = snippets.get(&engine, &result.hits[0], &ticket)?;
        assert!(
            snippet
                .highlights
                .iter()
                .any(|r| &snippet.fragment[r.clone()] == "Orphic")
        );
    }
    let multiple = engine.search("ancient orphi", &ticket)?;
    assert_eq!(multiple.hits.len(), 2);
    assert!(multiple.hits.iter().all(|h| h.tier == "prefix"));
    assert!(engine.search("\"orphi\"", &ticket)?.hits.is_empty());
    assert!(engine.search("unrelated orphi", &ticket)?.hits.is_empty());
    Ok(())
}

#[test]
fn title_ranking_precedes_body_result_limit_and_preserves_title_quality() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.note("Ilia", "Complete title.")?;
    fixture.note("Ilia biography", "Whole title word.")?;
    fixture.note("Iliad", "A title prefix.")?;
    fixture.note("Ailiad", "An embedded title substring.")?;
    for i in 0..120 {
        fixture.note(&format!("Account {i}"), &"Ilia ".repeat(100))?;
    }
    let engine = fixture.engine()?;
    let ticket = Requests::default().begin();
    let result = engine.search("ilia", &ticket)?;
    assert_eq!(result.hits.len(), crate::query::RESULT_LIMIT);
    assert_eq!(
        result.hits[..4]
            .iter()
            .map(|h| h.title.as_str())
            .collect::<Vec<_>>(),
        ["Ilia", "Ilia biography", "Iliad", "Ailiad"]
    );
    assert!(result.hits[4..].iter().all(|h| h.tier == "exact"));
    Ok(())
}

#[test]
fn late_matches_unicode_checkpoints_cache_staleness_and_cancellation() -> Result<()> {
    let fixture = Fixture::new()?;
    let body = format!(
        "{}東京 café luminous needle τέλος ordinary luminous",
        "ordinary filler ".repeat(100_000)
    );
    fixture.note("Big", &body)?;
    fixture.note(
        "Filtered",
        &format!("{} decisive checkpoint", "x".repeat(30_000)),
    )?;
    let engine = fixture.engine()?;
    let requests = Requests::default();
    let ticket = requests.begin();
    let mut snippets = Snippets::default();
    let result = engine.search("\"café luminous needle\"", &ticket)?;
    let hit = &result.hits[0];
    let first = snippets.get(&engine, hit, &ticket)?;
    assert_eq!(first.status, "matched");
    assert!(first.source_offset > 1_000_000);
    assert!(first.bytes_read <= WINDOW_BYTES);
    assert!(
        first
            .highlights
            .iter()
            .any(|r| &first.fragment[r.clone()] == "café luminous needle")
    );
    assert_eq!(
        &body[first.source_offset as usize..first.source_offset as usize + first.fragment.len()],
        first.fragment
    );
    // A common first word occurs >4096 times before the only matching phrase.
    let late_phrase = engine.search("\"ordinary luminous\"", &ticket)?;
    let late = snippets.get(&engine, &late_phrase.hits[0], &ticket)?;
    assert_eq!(late.status, "matched");
    assert!(late.source_offset > 1_000_000);
    let nearby = engine.search("ordinary luminous", &ticket)?;
    let late = snippets.get(&engine, &nearby.hits[0], &ticket)?;
    assert_eq!(late.status, "matched");
    assert!(late.fragment.contains("luminous"));
    let cached = snippets.get(&engine, hit, &ticket)?;
    assert!(cached.cached);
    assert_eq!(cached.bytes_read, 0);
    assert_eq!(cached.fragment, first.fragment);
    let long_token = engine.search("decisive checkpoint", &ticket)?;
    let excerpt = snippets.get(&engine, &long_token.hits[0], &ticket)?;
    assert_eq!(excerpt.status, "matched");
    assert!(excerpt.source_offset >= 30_000);
    fixture.note("Big", "File changed since indexing")?;
    assert_eq!(snippets.get(&engine, hit, &ticket)?.status, "stale");
    fs::remove_file(fixture.0.join("vault/Big.md"))?;
    assert_eq!(snippets.get(&engine, hit, &ticket)?.status, "missing");
    let _new = requests.begin();
    assert!(engine.search("decisive", &ticket).is_err());
    assert!(snippets.get(&engine, &long_token.hits[0], &ticket).is_err());
    Ok(())
}

#[test]
fn prefix_expansions_are_bounded_and_report_truncation() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.note(
        "Words",
        &(0..100)
            .map(|n| format!("prefixword{n} "))
            .collect::<String>(),
    )?;
    let engine = fixture.engine()?;
    let requests = Requests::default();
    let result = engine.search("prefix*", &requests.begin())?;
    assert!(result.expansions_truncated);
    assert_eq!(result.hits.len(), 1);
    assert!(result.hits[0].plan.groups[0].len() <= 16);
    Ok(())
}

#[test]
fn overlapping_phrase_highlights_form_disjoint_utf8_runs() -> Result<()> {
    let fixture = Fixture::new()?;
    fixture.note("Repeated", "foo foo foo")?;
    let engine = fixture.engine()?;
    let ticket = Requests::default().begin();
    let hits = engine.search("\"foo foo\"", &ticket)?.hits;
    let snippet = Snippets::default().get(&engine, &hits[0], &ticket)?;
    assert_eq!(snippet.highlights, vec![0..11]);
    assert_eq!(snippet.fragment, "foo foo foo");
    Ok(())
}
