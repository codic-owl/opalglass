//! Read-only measurements against a real or generated vault.
#[global_allocator]
static ALLOCATOR: opalglass::Allocator = opalglass::Allocator;
use opalglass::{
    search::{Requests, SearchIndex, Snippets},
    vault::Vault,
};
use std::{hint::black_box, sync::atomic::AtomicBool, time::Instant};

fn main() {
    let root = std::env::args_os().nth(1).expect("vault path required");
    let start = Instant::now();
    let mut vault = Vault::discover(root, &AtomicBool::new(false)).unwrap();
    println!(
        "paths ready: {:?}; notes: {}",
        start.elapsed(),
        vault.notes.len()
    );
    let start = Instant::now();
    vault.refresh().unwrap();
    println!("content and backlinks indexed: {:?}", start.elapsed());
    let start = Instant::now();
    vault.refresh().unwrap();
    println!("unchanged refresh: {:?}", start.elapsed());
    let path = vault
        .notes
        .first()
        .map(|note| note.path.as_path())
        .unwrap_or(std::path::Path::new(""));
    let start = Instant::now();
    for _ in 0..10 {
        black_box(vault.backlinks(path));
        black_box(&vault.notes);
    }
    println!("10 backlink + inventory queries: {:?}", start.elapsed());
    let start = Instant::now();
    let index = SearchIndex::open(&vault.root, None).unwrap();
    let paths = vault
        .notes
        .iter()
        .map(|n| n.path.clone())
        .collect::<Vec<_>>();
    let report = index.synchronize(&paths, &AtomicBool::new(false)).unwrap();
    println!(
        "temporary search index: {:?}; {:?}",
        start.elapsed(),
        report
    );
    let engine = index.snapshot().unwrap();
    let requests = Requests::default();
    let mut snippets = Snippets::default();
    for query in [
        "Glossary",
        "glossry",
        "divine light",
        "opalglass-profile-missing-term",
    ] {
        let ticket = requests.begin();
        let start = Instant::now();
        let result = engine.search(query, &ticket).unwrap();
        let search = start.elapsed();
        let start = Instant::now();
        for hit in result.hits.iter().take(5) {
            black_box(snippets.get(&engine, hit, &ticket).unwrap());
        }
        println!(
            "{query:?}: search {search:?}; five snippets {:?}; {} results",
            start.elapsed(),
            result.hits.len()
        );
    }
}
