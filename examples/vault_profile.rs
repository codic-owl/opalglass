//! Read-only measurements against a real or generated vault.
use opalglass::vault::Vault;
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
        black_box(vault.search(""));
    }
    println!("10 backlink + empty search queries: {:?}", start.elapsed());
    let start = Instant::now();
    black_box(vault.search("opalglass-profile-missing-term"));
    println!("full-text search: {:?}", start.elapsed());
}
