use crate::{
    Result, memory, phrase,
    search::{Engine, Requests, Ticket},
    snippets::Snippets,
};
use serde_json::{Value as Json, json};
use std::{
    fs,
    io::{self, Write},
    path::Path,
    time::Instant,
};
use tantivy::{query::QueryParser, snippet::SnippetGenerator};

const CASES: &[&str] = &[
    "Glossary",
    "Saducismus Triumphatus",
    "Hydriotaphia",
    "glossry",
    "compassion",
    "divine light",
    "opalglass-profile-missing-term",
    "glosssary",
    "glosasry",
    "compasion",
    "divnie light",
    "saducisums triumphatus",
    "\"divnie light\"",
    "gloss*",
    "xy",
    "hydriataphya",
];
fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}
fn stats(mut samples: Vec<f64>) -> Json {
    samples.sort_by(f64::total_cmp);
    json!({"median_ms": (samples[9]+samples[10])/2.0, "p95_ms": samples[18], "samples": 20})
}
fn snippets(
    engine: &Engine,
    hits: &[crate::search::Hit],
    cache: &mut Snippets,
    ticket: &Ticket,
) -> Result<Json> {
    let mut items = vec![];
    let mut bytes = 0;
    for hit in hits.iter().take(5) {
        let snippet = cache.get(engine, hit, ticket)?;
        bytes += snippet.bytes_read;
        let mut item = snippet.json(false);
        item["path"] = json!(hit.path);
        items.push(item);
    }
    Ok(json!({"source_bytes_read": bytes, "snippets": items}))
}
pub fn benchmark(directory: &Path) -> Result<Json> {
    let start = Instant::now();
    let engine = Engine::open(directory)?;
    let open_ms = ms(start);
    let after_open = memory();
    let requests = Requests::default();
    let mut cache = Snippets::default();
    let mut queries = vec![];
    for query in CASES {
        let ticket = requests.begin();
        let start = Instant::now();
        let result = engine.search(query, &ticket)?;
        let first_ms = ms(start);
        let mut times = vec![];
        for _ in 0..20 {
            let start = Instant::now();
            std::hint::black_box(engine.search(query, &ticket)?);
            times.push(ms(start));
        }
        cache.clear();
        let start = Instant::now();
        let excerpts = snippets(&engine, &result.hits, &mut cache, &ticket)?;
        let snippet_first_ms = ms(start);
        let mut uncached = vec![];
        for _ in 0..20 {
            cache.clear();
            let start = Instant::now();
            std::hint::black_box(snippets(&engine, &result.hits, &mut cache, &ticket)?);
            uncached.push(ms(start));
        }
        let mut cached = vec![];
        for _ in 0..20 {
            let start = Instant::now();
            std::hint::black_box(snippets(&engine, &result.hits, &mut cache, &ticket)?);
            cached.push(ms(start));
        }
        queries.push(json!({"query": query, "search_first_ms": first_ms, "search_warm": stats(times),
            "results": result.json(), "top5_snippets_first_ms": snippet_first_ms,
            "top5_snippets_uncached_warm": stats(uncached), "top5_snippets_cached": stats(cached), "excerpts": excerpts}));
    }
    let memory_before_full_file_baseline = memory();
    // Same queries, same five files and same index. Compare with the old full-file
    // snippet path, after recording bounded-path memory so baseline allocations don't contaminate it.
    let mut comparisons = vec![];
    let parser =
        QueryParser::for_index(&engine.index, vec![engine.fields.title, engine.fields.body]);
    for query in crate::QUERIES {
        let ticket = requests.begin();
        let hits = engine.search(&phrase(query), &ticket)?.hits;
        let query_object = parser.parse_query(&phrase(query))?;
        let mut generator =
            SnippetGenerator::create(&engine.searcher, &query_object, engine.fields.body)?;
        generator.set_max_num_chars(200);
        let mut old_bytes = 0;
        let start = Instant::now();
        for hit in hits.iter().take(5) {
            let path = engine.root.join(&hit.path).canonicalize()?;
            if !path.starts_with(&engine.root) {
                return Err("Baseline path escaped vault".into());
            }
            let body = fs::read_to_string(path)?;
            old_bytes += body.len();
            std::hint::black_box(generator.snippet(&body));
        }
        let old_ms = ms(start);
        cache.clear();
        let start = Instant::now();
        let bounded = snippets(&engine, &hits, &mut cache, &ticket)?;
        comparisons.push(
            json!({"query": query, "files": hits.len().min(5), "whole_file_ms": old_ms,
            "whole_file_bytes_read": old_bytes, "bounded_ms": ms(start), "bounded": bounded}),
        );
    }
    Ok(
        json!({"index_format": 2, "index_open_ms": open_ms, "memory_after_open": after_open,
        "memory_after_bounded_search_and_snippets": memory_before_full_file_baseline,
        "memory_after_whole_file_baseline": memory(), "queries": queries, "same_hits_snippet_comparison": comparisons,
        "method": "Fresh process; OS caches not cleared. Search: first + 20 warm calls, excluding snippets and JSON formatting. Snippets: top five visible results, first + 20 warm uncached + 20 cached calls. Comparison: one sample per path on identical literal-phrase hits. No GUI; no body copies stored."}),
    )
}
pub fn interactive(directory: &Path, query: &str) -> Result<()> {
    let engine = Engine::open(directory)?;
    let requests = Requests::default();
    let ticket = requests.begin();
    let start = Instant::now();
    let results = engine.search(query, &ticket)?;
    println!(
        "{}",
        json!({"event": "results", "elapsed_ms": ms(start), "results": results.json()})
    );
    io::stdout().flush()?;
    let mut cache = Snippets::default();
    for hit in results.hits.iter().take(5) {
        let start = Instant::now();
        let snippet = cache.get(&engine, hit, &ticket)?;
        println!(
            "{}",
            json!({"event": "snippet", "path": hit.path, "elapsed_ms": ms(start), "snippet": snippet.json(true)})
        );
        io::stdout().flush()?;
    }
    Ok(())
}
