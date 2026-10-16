//! Read-only backend memory audit. The counting allocator is only in this executable.
//! Usage: memory_profile VAULT [CACHE_DIRECTORY_OUTSIDE_VAULT]
//! Repeat with the same cache directory to measure an existing-index startup.
use opalglass::{search::SearchIndex, vault::Vault};
use std::{
    alloc::{GlobalAlloc, Layout},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::Instant,
};
struct Measured;
const INNER: opalglass::Allocator = opalglass::Allocator;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
fn allocated(size: usize) {
    let live = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(live, Ordering::Relaxed);
}
unsafe impl GlobalAlloc for Measured {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { INNER.alloc(layout) };
        if !p.is_null() {
            allocated(layout.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { INNER.alloc_zeroed(layout) };
        if !p.is_null() {
            allocated(layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        unsafe { INNER.dealloc(p, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let next = unsafe { INNER.realloc(p, layout, size) };
        if !next.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            allocated(size);
        }
        next
    }
}
#[global_allocator]
static ALLOCATOR: Measured = Measured;
fn report(phase: &str) {
    let live = LIVE.load(Ordering::Relaxed) as f64 / 1048576.;
    let peak = PEAK.load(Ordering::Relaxed) as f64 / 1048576.;
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let rss = status
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .unwrap_or("RSS unavailable");
    println!("{phase}: live={live:.1} MiB peak_live={peak:.1} MiB {rss}");
}
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut args = std::env::args_os().skip(1);
    let root = args.next().expect("vault path required");
    let cache = args.next().map(std::path::PathBuf::from);
    let started = Instant::now();
    let mut vault = Vault::discover(root, &AtomicBool::new(false))?;
    report("discovery");
    vault.refresh()?;
    report("backlinks");
    let paths = vault
        .notes
        .iter()
        .map(|n| n.path.clone())
        .collect::<Vec<_>>();
    let index = SearchIndex::open(&vault.root, cache.as_deref())?;
    report("index opened");
    println!(
        "sync={:?}",
        index.synchronize(&paths, &AtomicBool::new(false))?
    );
    report("indexed");
    let engine = index.snapshot()?;
    report("snapshot");
    std::thread::sleep(std::time::Duration::from_secs(3));
    report("settled");
    drop(index);
    std::thread::sleep(std::time::Duration::from_secs(1));
    report("index dropped");
    drop(engine);
    drop(vault);
    report("all dropped");
    println!("elapsed={:?}", started.elapsed());
    Ok(())
}
