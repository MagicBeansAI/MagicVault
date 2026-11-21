//! Heap a captured entry keeps resident, measured through the public API.
//!
//! Its own test binary: the counting allocator is process-global, so a second
//! test running in parallel would be counted too. One test, synthetic values.
use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};

use magicvault_core::{store::SecretStore, MasterKeyProvider};

struct Counting;

static BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static LIVE_ALLOCATIONS: AtomicIsize = AtomicIsize::new(0);

fn grew(by: usize) {
    let now = BYTES.fetch_add(by, Ordering::SeqCst) + by;
    PEAK.fetch_max(now, Ordering::SeqCst);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = System.alloc(layout);
        if !pointer.is_null() {
            grew(layout.size());
            LIVE_ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = System.alloc_zeroed(layout);
        if !pointer.is_null() {
            grew(layout.size());
            LIVE_ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
        BYTES.fetch_sub(layout.size(), Ordering::SeqCst);
        LIVE_ALLOCATIONS.fetch_sub(1, Ordering::SeqCst);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let moved = System.realloc(pointer, layout, new_size);
        if !moved.is_null() {
            if new_size >= layout.size() {
                grew(new_size - layout.size());
            } else {
                BYTES.fetch_sub(layout.size() - new_size, Ordering::SeqCst);
            }
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const FIELDS: usize = 10_000;

/// Same key provider across reopen, so the reloaded store can decrypt.
struct FixedKey;

impl MasterKeyProvider for FixedKey {
    fn get_or_create_key(
        &self,
    ) -> Result<[u8; 32], magicvault_core::encryption::SecretEncryptionError> {
        Ok([7u8; 32])
    }

    fn delete_key(&self) -> Result<(), magicvault_core::encryption::SecretEncryptionError> {
        Ok(())
    }

    fn provider_name(&self) -> &str {
        "synthetic-resident-fixture"
    }
}

fn storage() -> HashMap<String, String> {
    // ~100 bytes of key and value per field, like browser storage.
    (0..FIELDS)
        .map(|field| {
            (
                format!("placeholder-key-{field}"),
                format!(
                    "{{\"synthetic\":\"PLACEHOLDER-{field}\",\"pad\":\"{}\"}}",
                    "x".repeat(40)
                ),
            )
        })
        .collect()
}

fn live() -> (usize, isize) {
    (
        BYTES.load(Ordering::SeqCst),
        LIVE_ALLOCATIONS.load(Ordering::SeqCst),
    )
}

#[test]
fn a_captured_entry_keeps_about_its_payload_resident() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().join("secrets");
    let payload = storage()
        .iter()
        .map(|(key, value)| key.len() + value.len())
        .sum::<usize>();

    // The plain map of full keys an entry used to hold, for comparison.
    let (before_map, before_map_allocations) = live();
    let plain = storage()
        .into_iter()
        .map(|(key, mut value)| {
            // Exact capacities, as strings parsed from the file have.
            let mut key = format!("local_storage:{key}");
            key.shrink_to_fit();
            value.shrink_to_fit();
            (key, value)
        })
        .collect::<HashMap<_, _>>();
    let (after_map, after_map_allocations) = live();
    let plain_bytes = after_map - before_map;
    let plain_allocations = after_map_allocations - before_map_allocations;
    drop(plain);

    let store = SecretStore::new_empty(Box::new(FixedKey), base.clone());
    // Warm up one-time lazies (tracing, JWT detection) outside the measurement.
    store
        .store_captured_deferred(
            "https://warm-up.invalid",
            HashMap::new(),
            Vec::new(),
            HashMap::from([("k".to_string(), "PLACEHOLDER".to_string())]),
            HashMap::new(),
        )
        .unwrap();

    let (before, before_allocations) = live();
    store
        .store_captured_deferred(
            "https://placeholder.invalid",
            HashMap::new(),
            Vec::new(),
            storage(),
            HashMap::new(),
        )
        .unwrap();
    let (after, after_allocations) = live();
    let resident = after - before;
    let allocations = after_allocations - before_allocations;
    store.flush_captured().unwrap();
    drop(store);

    let (before_load, before_load_allocations) = live();
    PEAK.store(before_load, Ordering::SeqCst);
    let reloaded = SecretStore::new(Box::new(FixedKey), base.clone()).unwrap();
    let (after_load, after_load_allocations) = live();
    let load_peak = PEAK.load(Ordering::SeqCst) - before_load;
    let file_bytes = std::fs::metadata(base.join("captured_secrets.vault"))
        .unwrap()
        .len() as usize;
    assert!(reloaded.has_captured("https://placeholder.invalid"));

    println!(
        "{FIELDS} fields, payload {payload} B ({:.1} B/field) | plain map {plain_bytes} B ({:.1} B/field), {plain_allocations} allocations | \
         compact entry {resident} B ({:.1} B/field), {allocations} allocations | \
         reloaded store {} B, {} allocations, load peak {load_peak} B for a {file_bytes} B file",
        payload as f64 / FIELDS as f64,
        plain_bytes as f64 / FIELDS as f64,
        resident as f64 / FIELDS as f64,
        after_load - before_load,
        after_load_allocations - before_load_allocations,
    );
    assert!(
        resident <= payload + 24 * FIELDS,
        "a captured field must cost its payload plus a slot: {resident} B for {payload} B of payload"
    );
    assert!(
        allocations <= 16,
        "an entry is a handful of allocations, not two per field: {allocations}"
    );
}
