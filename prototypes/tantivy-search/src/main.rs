//! Standalone, read-only vault benchmark. Body text is indexed, never STORED.
mod search;
mod snippets;
#[cfg(test)]
mod tests_v2;
mod trial;
use serde_json::{Value as Json, json};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};
use tantivy::{
    DocAddress, Index, IndexWriter, Searcher, TantivyDocument, Term,
    collector::{Count, TopDocs},
    doc,
    indexer::IndexWriterOptions,
    query::{QueryParser, TermQuery},
    schema::{Field, IndexRecordOption, STORED, STRING, Schema, TEXT, Value},
    snippet::SnippetGenerator,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const WRITER_BYTES: usize = 32 * 1024 * 1024;
const INPUT_BATCH_BYTES: usize = 16 * 1024 * 1024;
const QUERIES: &[&str] = &[
    "Glossary",
    "Saducismus Triumphatus",
    "Hydriotaphia",
    "glossry",
    "compassion",
    "divine light",
    "opalglass-profile-missing-term",
];

#[derive(Clone, Copy)]
struct Fields {
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
    let mut b = Schema::builder();
    b.add_text_field("path", STRING | STORED);
    b.add_text_field("title", TEXT | STORED);
    b.add_text_field("exact_title", STRING);
    b.add_text_field("body", TEXT); // Deliberately no STORED flag.
    b.add_bytes_field("checkpoints", STORED);
    b.add_u64_field("source_len", STORED);
    b.add_text_field("source_mtime", STORED);
    b.build()
}
fn writer(index: &Index) -> Result<IndexWriter> {
    Ok(index.writer_with_options(
        IndexWriterOptions::builder()
            .num_worker_threads(1)
            .num_merge_threads(1)
            .memory_budget_per_thread(WRITER_BYTES)
            .build(),
    )?)
}
fn paths(dir: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            paths(&entry.path(), output)?;
        } else if kind.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            output.push(entry.path());
        }
    }
    Ok(())
}
fn disk_bytes(dir: &Path) -> Result<u64> {
    let mut total = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        total += if entry.file_type()?.is_dir() {
            disk_bytes(&entry.path())?
        } else {
            entry.metadata()?.len()
        };
    }
    Ok(total)
}
fn memory() -> Json {
    // Linux process metrics; null on other operating systems. No platform-specific build flags.
    let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
    let mib = |key: &str| -> Option<f64> {
        status
            .lines()
            .find_map(|l| l.strip_prefix(key))?
            .split_whitespace()
            .next()?
            .parse::<f64>()
            .ok()
            .map(|n| n / 1024.0)
    };
    json!({"rss_mib": mib("VmRSS:"), "peak_rss_mib": mib("VmHWM:")})
}
fn build(root: &Path, destination: &Path) -> Result<Json> {
    let start = Instant::now();
    let root = root.canonicalize()?;
    let mut files = vec![];
    paths(&root, &mut files)?;
    files.sort();
    // Refuse to replace any existing directory or index.
    fs::create_dir(destination)?;
    let destination = destination.canonicalize()?;
    if destination.starts_with(&root) {
        fs::remove_dir(&destination)?;
        return Err("Index must be outside the vault".into());
    }
    let schema = schema();
    let f = Fields::from(&schema)?;
    let index = Index::create_in_dir(&destination, schema)?;
    let mut writer = writer(&index)?;
    let mut total_bytes = 0;
    let mut batch_bytes = 0;
    let mut checkpoint_bytes = 0;
    let mut tokenizer = index.tokenizer_for_field(f.body)?;
    for path in &files {
        let version = snippets::FileVersion::read(path)?;
        let text = fs::read_to_string(path)?;
        if version != snippets::FileVersion::read(path)? || version.len != text.len() as u64 {
            return Err(format!("Source changed while indexing: {}", path.display()).into());
        }
        let checkpoints = snippets::checkpoints(&mut tokenizer, &text);
        checkpoint_bytes += checkpoints.len();
        let title = path.file_stem().ok_or("Missing title")?.to_string_lossy();
        total_bytes += text.len();
        batch_bytes += text.len();
        writer.add_document(
            doc!(f.path => path.strip_prefix(&root)?.to_string_lossy().as_ref(),
            f.title => title.as_ref(), f.exact => title.to_lowercase(), f.body => text.as_str(),
            f.checkpoints => checkpoints, f.source_len => version.len, f.source_mtime => version.modified),
        )?;
        // Drain the input queue periodically: the writer arena budget alone does not bound queued bodies.
        if batch_bytes >= INPUT_BATCH_BYTES {
            writer.commit()?;
            batch_bytes = 0;
        }
    }
    writer.commit()?;
    writer.wait_merging_threads()?;
    let index_ms = start.elapsed().as_secs_f64() * 1000.0;
    fs::write(
        destination.join("source.json"),
        serde_json::to_vec_pretty(
            &json!({"format": 2, "root": root, "notes": files.len(), "bytes": total_bytes}),
        )?,
    )?;
    let reader = index.reader()?;
    let searcher = reader.searcher();
    if searcher.num_docs() != files.len() as u64 {
        return Err("Indexed document count mismatch".into());
    }
    // Verify every stored document, not just the schema declaration.
    for (segment_ord, segment) in searcher.segment_readers().iter().enumerate() {
        for doc_id in 0..segment.max_doc() {
            let document =
                searcher.doc::<TantivyDocument>(DocAddress::new(segment_ord as u32, doc_id))?;
            if document.get_first(f.body).is_some() {
                return Err("Unexpected stored body".into());
            }
        }
    }
    Ok(
        json!({"tantivy": "0.26.2", "notes": files.len(), "source_bytes": total_bytes,
        "index_build_ms_including_commits_and_merges": index_ms, "index_bytes": disk_bytes(&destination)?,
        "segments": searcher.segment_readers().len(), "worker_threads": 1, "merge_threads": 1,
        "writer_arena_bytes": WRITER_BYTES, "input_batch_bytes": INPUT_BATCH_BYTES,
        "checkpoint_bytes_before_compression": checkpoint_bytes,
        "stored_bodies": 0, "all_stored_documents_checked": true, "memory_after_verification": memory()}),
    )
}
fn phrase(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}
fn text(doc: &TantivyDocument, field: Field) -> String {
    doc.get_first(field)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned()
}
fn rank(searcher: &Searcher, parser: &QueryParser, f: Fields, query: &str) -> Result<Json> {
    let parsed = parser.parse_query(&phrase(query))?;
    let (raw, count) =
        searcher.search(&parsed, &(TopDocs::with_limit(20).order_by_score(), Count))?;
    let exact = TermQuery::new(
        Term::from_field_text(f.exact, &query.to_lowercase()),
        IndexRecordOption::Basic,
    );
    let exact_hits = searcher.search(&exact, &TopDocs::with_limit(20).order_by_score())?;
    let mut addresses = vec![];
    for (_, address) in exact_hits.iter().chain(raw.iter()) {
        if !addresses.contains(address) {
            addresses.push(*address);
        }
    }
    addresses.truncate(20);
    let mut hits = vec![];
    for address in addresses {
        let doc = searcher.doc::<TantivyDocument>(address)?;
        if doc.get_first(f.body).is_some() {
            return Err("Unexpected stored body".into());
        }
        hits.push(json!({"path": text(&doc, f.path), "title": text(&doc, f.title)}));
    }
    let mut raw_exact_rank = None;
    for (i, (_, address)) in raw.iter().enumerate() {
        let doc = searcher.doc::<TantivyDocument>(*address)?;
        if text(&doc, f.title).to_lowercase() == query.to_lowercase() {
            raw_exact_rank = Some(i + 1);
            break;
        }
    }
    Ok(
        json!({"token_phrase_hit_count": count, "exact_title_hits": exact_hits.len(),
        "raw_bm25_exact_title_rank_in_top20": raw_exact_rank,
        "exact_title_rank_after_priority": hits.iter().position(|h| h["title"].as_str().unwrap().to_lowercase() == query.to_lowercase()).map(|i| i + 1),
        "hits": hits}),
    )
}
fn benchmark(directory: &Path) -> Result<Json> {
    let start = Instant::now();
    let index = Index::open_in_dir(directory)?;
    let f = Fields::from(&index.schema())?;
    let reader = index.reader()?;
    let searcher = reader.searcher();
    let open_ms = start.elapsed().as_secs_f64() * 1000.0;
    let before = memory();
    let source: Json = serde_json::from_slice(&fs::read(directory.join("source.json"))?)?;
    let root =
        PathBuf::from(source["root"].as_str().ok_or("Invalid source root")?).canonicalize()?;
    let mut parser = QueryParser::for_index(&index, vec![f.title, f.body]);
    parser.set_field_boost(f.title, 10.0);
    let mut output = vec![];
    for query in QUERIES {
        let start = Instant::now();
        let results = rank(&searcher, &parser, f, query)?;
        let first_ms = start.elapsed().as_secs_f64() * 1000.0;
        let mut warm = vec![];
        for _ in 0..20 {
            let start = Instant::now();
            std::hint::black_box(rank(&searcher, &parser, f, query)?);
            warm.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        warm.sort_by(f64::total_cmp);
        let parsed = parser.parse_query(&phrase(query))?;
        let mut generator = SnippetGenerator::create(&searcher, &parsed, f.body)?;
        generator.set_max_num_chars(200);
        let before_snippets = Instant::now();
        let mut snippets = vec![];
        let mut bytes_read = 0;
        for hit in results["hits"].as_array().unwrap().iter().take(5) {
            let path = root.join(hit["path"].as_str().unwrap()).canonicalize()?;
            if !path.starts_with(&root) {
                return Err("Snippet path escaped vault".into());
            }
            let body = fs::read_to_string(path)?;
            bytes_read += body.len();
            let snippet = generator.snippet(&body);
            // Keep the report free of copied note content; validate highlighting and record its size.
            snippets.push(json!({"path": hit["path"], "fragment_bytes": snippet.fragment().len(), "highlight_count": snippet.highlighted().len()}));
        }
        output.push(json!({"query": query, "first_call_ms": first_ms,
            "warm_median_ms": (warm[9] + warm[10]) / 2.0, "warm_p95_ms": warm[18],
            "warm_samples": 20, "results": results, "top5_disk_snippets_ms": before_snippets.elapsed().as_secs_f64() * 1000.0,
            "snippet_source_bytes_read": bytes_read, "snippets": snippets}));
    }
    Ok(
        json!({"notes": searcher.num_docs(), "segments": searcher.segment_readers().len(),
        "index_open_ms": open_ms, "memory_after_open": before, "memory_after_queries_and_snippets": memory(),
        "query_semantics": "literal token phrases; exact lowercase title priority; title BM25 weight 10; no fuzzy matching",
        "timing_scope": "parse, top20+count, exact-title lookup, metadata fetch, ranking diagnostics and JSON construction; excludes snippets and UI",
        "queries": output}),
    )
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let result =
        match args.as_slice() {
            [mode, vault, directory] if mode == "build" => {
                build(Path::new(vault), Path::new(directory))?
            }
            [mode, directory] if mode == "bench" => benchmark(Path::new(directory))?,
            [mode, directory] if mode == "bench-v2" => trial::benchmark(Path::new(directory))?,
            [mode, directory, query] if mode == "search" => {
                trial::interactive(Path::new(directory), &query.to_string_lossy())?;
                return Ok(());
            }
            _ => return Err(
                "Usage: opalglass-tantivy-prototype build VAULT NEW_INDEX_DIR | bench INDEX_DIR | bench-v2 INDEX_DIR | search INDEX_DIR QUERY"
                    .into(),
            ),
        };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bodies_are_searchable_but_not_stored_and_exact_titles_win() -> Result<()> {
        let index = Index::create_in_ram(schema());
        let f = Fields::from(&index.schema())?;
        let mut writer = writer(&index)?;
        writer.add_document(doc!(f.path => "Glossary.md", f.title => "Glossary", f.exact => "glossary", f.body => "divine light ".repeat(1000)))?;
        writer.add_document(doc!(f.path => "Other.md", f.title => "Other", f.exact => "other", f.body => "Glossary light divine"))?;
        writer.commit()?;
        writer.wait_merging_threads()?;
        let reader = index.reader()?;
        let searcher = reader.searcher();
        let parser = QueryParser::for_index(&index, vec![f.title, f.body]);
        let results = rank(&searcher, &parser, f, "GLOSSARY")?;
        assert_eq!(results["hits"][0]["path"], "Glossary.md");
        let query = parser.parse_query(&phrase("divine light"))?;
        let hits = searcher.search(&query, &TopDocs::with_limit(5).order_by_score())?;
        assert_eq!(hits.len(), 1); // Word order/positions matter.
        let doc = searcher.doc::<TantivyDocument>(hits[0].1)?;
        assert_eq!(text(&doc, f.path), "Glossary.md");
        assert!(doc.get_first(f.body).is_none());
        let snippet = SnippetGenerator::create(&searcher, &query, f.body)?.snippet("divine light");
        assert!(!snippet.highlighted().is_empty());
        Ok(())
    }
}
