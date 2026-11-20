//! How long a large captured flush makes other callers wait.
//!
//! A measurement, not a gate: it builds a partition the size of a real browser
//! capture store (~45 MB on disk), so it is ignored by default. Run it with
//! `cargo test -p magicvault-core --test flush_latency -- --ignored --nocapture`.
//! The worst wait of a writer on another origin bounds how long the flush holds
//! the state lock; a reader is measured too, since a queued writer can hold
//! readers back behind it. Synthetic values only.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use magicvault_core::{store::SecretStore, InMemoryKeyProvider};

const ORIGINS: usize = 360;
const FIELDS_PER_ORIGIN: usize = 1_000;
const PROBE: &str = "https://probe.invalid";

fn probe_write(store: &SecretStore) {
    store
        .store_captured_deferred(
            PROBE,
            HashMap::from([("Authorization".to_string(), "Bearer PLACEHOLDER-PROBE".to_string())]),
            Vec::new(),
            HashMap::new(),
            HashMap::new(),
        )
        .unwrap();
}

/// Call the store on another origin until `done`, returning the worst wait.
fn worst(
    store: Arc<SecretStore>,
    done: Arc<AtomicBool>,
    started: Arc<AtomicBool>,
    write: bool,
) -> thread::JoinHandle<(Duration, u64)> {
    thread::spawn(move || {
        let (mut worst, mut calls) = (Duration::ZERO, 0u64);
        while !started.load(Ordering::SeqCst) {
            std::hint::spin_loop();
        }
        while !done.load(Ordering::SeqCst) {
            let call = Instant::now();
            if write {
                probe_write(&store);
            } else {
                let _ = store.get_session(PROBE, "https://probe.invalid/");
            }
            worst = worst.max(call.elapsed());
            calls += 1;
        }
        (worst, calls)
    })
}

#[test]
#[ignore = "measurement: builds a ~45 MB captured partition"]
fn callers_wait_during_a_large_captured_flush() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().join("secrets");
    let store = Arc::new(SecretStore::new_empty(Box::new(InMemoryKeyProvider::new()), base.clone()));
    for origin in 0..ORIGINS {
        let storage = (0..FIELDS_PER_ORIGIN)
            .map(|field| {
                (
                    format!("placeholder-key-{field}"),
                    format!("{{\"synthetic\":\"PLACEHOLDER-{origin}-{field}\",\"pad\":\"{}\"}}", "x".repeat(30)),
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
    probe_write(&store);

    // The noise floor: the same callers for a while with no flush running.
    let idle_done = Arc::new(AtomicBool::new(false));
    let idle_started = Arc::new(AtomicBool::new(true));
    let idle_writer = worst(Arc::clone(&store), Arc::clone(&idle_done), Arc::clone(&idle_started), true);
    let idle_reader = worst(Arc::clone(&store), Arc::clone(&idle_done), Arc::clone(&idle_started), false);
    thread::sleep(Duration::from_secs(10));
    idle_done.store(true, Ordering::SeqCst);
    let (idle_write, _) = idle_writer.join().unwrap();
    let (idle_read, _) = idle_reader.join().unwrap();
    println!("no flush running: worst writer wait {idle_write:?}, worst reader wait {idle_read:?}");

    let done = Arc::new(AtomicBool::new(false));
    let started = Arc::new(AtomicBool::new(false));
    let writer = worst(Arc::clone(&store), Arc::clone(&done), Arc::clone(&started), true);
    let reader = worst(Arc::clone(&store), Arc::clone(&done), Arc::clone(&started), false);

    started.store(true, Ordering::SeqCst);
    let flush = Instant::now();
    store.flush_captured().unwrap();
    let flush_time = flush.elapsed();
    done.store(true, Ordering::SeqCst);
    let (worst_write, writes) = writer.join().unwrap();
    let (worst_read, reads) = reader.join().unwrap();
    let file_bytes = std::fs::metadata(base.join("captured_secrets.vault")).unwrap().len();
    println!(
        "captured flush of {file_bytes} B: {flush_time:?}; worst writer wait {worst_write:?} over {writes} writes; worst reader wait {worst_read:?} over {reads} reads"
    );
}
