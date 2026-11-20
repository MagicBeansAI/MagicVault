//! Peak heap a captured flush needs above the store it flushes.
//!
//! Its own test binary: the counting allocator is process-global, so a second
//! test running in parallel would be counted too. One test, synthetic values.
use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use magicvault_core::{store::SecretStore, InMemoryKeyProvider};

struct Counting;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grew(by: usize) {
    let now = CURRENT.fetch_add(by, Ordering::SeqCst) + by;
    PEAK.fetch_max(now, Ordering::SeqCst);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = System.alloc(layout);
        if !pointer.is_null() {
            grew(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = System.alloc_zeroed(layout);
        if !pointer.is_null() {
            grew(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
        CURRENT.fetch_sub(layout.size(), Ordering::SeqCst);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let moved = System.realloc(pointer, layout, new_size);
        if !moved.is_null() {
            if new_size >= layout.size() {
                grew(new_size - layout.size());
            } else {
                CURRENT.fetch_sub(layout.size() - new_size, Ordering::SeqCst);
            }
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const ORIGINS: usize = 40;
const FIELDS_PER_ORIGIN: usize = 2_000;

#[test]
fn a_captured_flush_holds_little_more_than_its_own_file() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().join("secrets");
    let store = SecretStore::new_empty(Box::new(InMemoryKeyProvider::new()), base.clone());
    for origin in 0..ORIGINS {
        // ~100 bytes of key and value per field, like browser storage.
        let storage = (0..FIELDS_PER_ORIGIN)
            .map(|field| {
                (
                    format!("placeholder-key-{field}"),
                    format!("{{\"synthetic\":\"PLACEHOLDER-{origin}-{field}\",\"pad\":\"{}\"}}", "x".repeat(40)),
                )
            })
            .collect::<HashMap<_, _>>();
        store
            .store_captured_deferred(
                &format!("https://placeholder-{origin}.invalid"),
                HashMap::new(),
                Vec::new(),
                storage,
                HashMap::new(),
            )
            .unwrap();
    }

    let baseline = CURRENT.load(Ordering::SeqCst);
    PEAK.store(baseline, Ordering::SeqCst);
    store.flush_captured().unwrap();
    let peak_above = PEAK.load(Ordering::SeqCst) - baseline;
    let file_bytes = std::fs::metadata(base.join("captured_secrets.vault")).unwrap().len() as usize;
    let ratio = peak_above as f64 / file_bytes as f64;
    println!(
        "captured flush: file {file_bytes} B, store heap {baseline} B, peak above store {peak_above} B ({ratio:.2}x the file)"
    );
    assert!(
        ratio < 1.6,
        "a flush must not hold copies of the partition: peak {peak_above} B for a {file_bytes} B file ({ratio:.2}x)"
    );
}
