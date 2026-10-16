//! Read-only fresh-index timing. Uses an isolated temporary cache on every run.
//! Usage: cargo run --release --example indexing_profile -- VAULT
use opalglass::{
    search::{Requests, SearchIndex, SyncProgress},
    vault::Vault,
};
use std::{sync::atomic::AtomicBool, time::Instant};

#[global_allocator]
static ALLOCATOR: opalglass::Allocator = opalglass::Allocator;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let root = std::env::args_os().nth(1).expect("vault path required");
    let start = Instant::now();
    let cancelled = AtomicBool::new(false);
    let mut vault = Vault::discover(root, &cancelled)?;
    let discovery = start.elapsed();
    let start = Instant::now();
    vault.refresh()?;
    let backlinks = start.elapsed();
    let paths: Vec<_> = vault.notes.iter().map(|n| n.path.clone()).collect();
    let index = SearchIndex::open(&vault.root, None)?;
    let start = Instant::now();
    let mut finalizing_at = std::time::Duration::ZERO;
    let report = index.synchronize_with_progress(&paths, &cancelled, |progress| {
        if progress == SyncProgress::Finalizing {
            finalizing_at = start.elapsed();
        }
    })?;
    let indexing = start.elapsed();
    let start = Instant::now();
    let engine = index.snapshot()?;
    let snapshot = start.elapsed();
    let start = Instant::now();
    let unchanged = index.synchronize(&paths, &cancelled)?;
    let refresh = start.elapsed();
    assert_eq!(unchanged.updated, 0);
    let requests = Requests::default();
    let mut searches = vec![];
    for query in ["Ilia", "orphi", "divine light", "glossry"] {
        let start = Instant::now();
        let results = engine.search(query, &requests.begin())?;
        searches.push(serde_json::json!({
            "query": query, "ms": start.elapsed().as_secs_f64() * 1000.,
            "hits": results.hits.len(),
        }));
    }
    // Linux-only measurement; the benchmark itself also runs on other platforms.
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let memory_kib = |name: &str| -> Option<u64> {
        status
            .lines()
            .find(|l| l.starts_with(name))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    };
    println!(
        "{}",
        serde_json::json!({
            "notes": report.notes,
            "discovery_s": discovery.as_secs_f64(),
            "backlinks_s": backlinks.as_secs_f64(),
            "indexing_s": indexing.as_secs_f64(),
            "finalizing_s": (indexing - finalizing_at).as_secs_f64(),
            "snapshot_s": snapshot.as_secs_f64(),
            "unchanged_s": refresh.as_secs_f64(),
            "peak_rss_kib": memory_kib("VmHWM:"),
            "rss_kib": memory_kib("VmRSS:"),
            "searches": searches,
        })
    );
    Ok(())
}
