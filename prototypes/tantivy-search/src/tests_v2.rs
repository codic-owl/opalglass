use crate::{
    Result, build,
    search::{Engine, Requests},
    snippets::{Snippets, WINDOW_BYTES},
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
struct Fixture(PathBuf);
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
        build(&self.0.join("vault"), &self.0.join("index"))?;
        Engine::open(&self.0.join("index"))
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
    assert!(result.hits.iter().any(|h| h.title == "Glossary"));
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
