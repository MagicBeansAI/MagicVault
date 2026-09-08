//! Bounded, opt-in counters for CDP-reported processes in our disposable browsers.
//! No command lines, environment, memory contents, personal process enumeration,
//! daemon sampling, or signals. Native hosts are NOT part of this CDP population.
use magicvault_test_support::browser::CdpPeer;
use serde_json::{json, Value};
use std::{collections::BTreeMap, time::Instant};

const MAX_PROCESSES: usize = 512;
type Identity = (i32, u64); // PID plus macOS process start time, never printed.

#[derive(Clone, Copy)]
struct Counter {
    cpu_seconds: f64,
    resident: u64,
    footprint: u64,
}
#[derive(Default)]
pub(super) struct Snapshot {
    processes: BTreeMap<Identity, Counter>,
    missing: usize,
}

pub(super) async fn sample(browsers: [(&mut CdpPeer, u32); 2]) -> Snapshot {
    let mut snapshot = Snapshot::default();
    for (peer, owned_pid) in browsers {
        let result = peer
            .command("SystemInfo.getProcessInfo", json!({}), None)
            .await;
        let rows = result["processInfo"]
            .as_array()
            .expect("CDP process counters");
        assert!(!rows.is_empty() && rows.len() <= MAX_PROCESSES / 2);
        assert!(rows.iter().any(|row| row["type"] == "browser" && row["id"].as_u64() == Some(owned_pid.into())), "resource peer must belong to the fixture's browser child");
        for row in rows {
            let pid = row["id"]
                .as_i64()
                .and_then(|id| i32::try_from(id).ok())
                .filter(|id| *id > 1)
                .expect("valid browser-owned PID");
            let cpu_seconds = row["cpuTime"]
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .expect("finite CDP CPU seconds");
            let mut buffer = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
            // SAFETY: RUSAGE_INFO_V2 selects this exact writable buffer. The PID
            // came from the fixture CDP peer, never a global process search.
            let status = unsafe {
                libc::proc_pid_rusage(
                    pid,
                    libc::RUSAGE_INFO_V2,
                    buffer.as_mut_ptr().cast::<libc::rusage_info_t>(),
                )
            };
            if status != 0 {
                snapshot.missing += 1; // A disappearing child is missing evidence, not zero usage.
                continue;
            }
            let info = unsafe { buffer.assume_init() };
            if info.ri_proc_start_abstime == 0 || info.ri_proc_exit_abstime != 0 {
                snapshot.missing += 1;
                continue;
            }
            assert!(
                snapshot
                    .processes
                    .insert(
                        (pid, info.ri_proc_start_abstime),
                        Counter {
                            cpu_seconds,
                            resident: info.ri_resident_size,
                            footprint: info.ri_phys_footprint,
                        }
                    )
                    .is_none(),
                "disposable browser process populations must be independent"
            );
        }
    }
    assert!(!snapshot.processes.is_empty() && snapshot.processes.len() <= MAX_PROCESSES);
    snapshot
}

pub(super) struct Window {
    started: Instant,
    previous: Snapshot,
    samples: usize,
    missing: usize,
    counter_regressions: usize,
    new_identities: usize,
    departed_identities: usize,
    matched_cpu_seconds: f64,
    first_resident: u64,
    first_footprint: u64,
    last_resident: u64,
    last_footprint: u64,
    peak_resident: u64,
    peak_footprint: u64,
    peak_processes: usize,
}

fn totals(snapshot: &Snapshot) -> (u64, u64) {
    snapshot
        .processes
        .values()
        .fold((0, 0), |(rss, footprint), c| {
            (rss + c.resident, footprint + c.footprint)
        })
}

impl Window {
    pub(super) fn new(first: Snapshot) -> Self {
        let (resident, footprint) = totals(&first);
        Self {
            started: Instant::now(),
            samples: 1,
            missing: first.missing,
            counter_regressions: 0,
            new_identities: 0,
            departed_identities: 0,
            matched_cpu_seconds: 0.0,
            first_resident: resident,
            first_footprint: footprint,
            last_resident: resident,
            last_footprint: footprint,
            peak_resident: resident,
            peak_footprint: footprint,
            peak_processes: first.processes.len(),
            previous: first,
        }
    }
    pub(super) fn observe(&mut self, mut next: Snapshot) {
        for (id, counter) in &mut next.processes {
            match self.previous.processes.get(id) {
                Some(before) if counter.cpu_seconds >= before.cpu_seconds => {
                    self.matched_cpu_seconds += counter.cpu_seconds - before.cpu_seconds
                }
                Some(before) => {
                    self.counter_regressions += 1;
                    // Keep the previous high-water value; a later recovery must
                    // not count CPU that was already included before regression.
                    counter.cpu_seconds = before.cpu_seconds;
                }
                None => self.new_identities += 1,
            }
        }
        self.departed_identities += self
            .previous
            .processes
            .keys()
            .filter(|id| !next.processes.contains_key(id))
            .count();
        (self.last_resident, self.last_footprint) = totals(&next);
        self.peak_resident = self.peak_resident.max(self.last_resident);
        self.peak_footprint = self.peak_footprint.max(self.last_footprint);
        self.peak_processes = self.peak_processes.max(next.processes.len());
        self.samples += 1;
        self.missing += next.missing;
        self.previous = next;
    }
    pub(super) fn report(&self, phase: &str) -> Value {
        assert!(matches!(phase, "active" | "idle"));
        json!({"phase":phase,"synthetic_consent":true,"profiles":2,
            "elapsed_ms":self.started.elapsed().as_millis(),"samples":self.samples,
            "matched_cpu_seconds":self.matched_cpu_seconds,
            "resident_kib":{"first":self.first_resident/1024,"last":self.last_resident/1024,"sampled_peak":self.peak_resident/1024},
            "footprint_kib":{"first":self.first_footprint/1024,"last":self.last_footprint/1024,"sampled_peak":self.peak_footprint/1024},
            "last_processes":self.previous.processes.len(),"peak_processes":self.peak_processes,
            "missing_observations":self.missing,"new_identities":self.new_identities,
            "departed_identities":self.departed_identities,"counter_regressions":self.counter_regressions})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(rows: &[(i32, u64, f64, u64)], missing: usize) -> Snapshot {
        Snapshot {
            processes: rows
                .iter()
                .map(|(pid, start, cpu, rss)| {
                    (
                        (*pid, *start),
                        Counter {
                            cpu_seconds: *cpu,
                            resident: *rss,
                            footprint: *rss,
                        },
                    )
                })
                .collect(),
            missing,
        }
    }
    #[test]
    fn reused_pids_missing_samples_and_counter_regressions_never_inflate_cpu() {
        let mut window = Window::new(snapshot(&[(2, 1, 5.0, 1024), (3, 1, 2.0, 1024)], 0));
        window.observe(snapshot(&[(2, 2, 500.0, 1024), (3, 1, 3.0, 2048)], 1));
        window.observe(snapshot(&[(2, 2, 499.0, 1024)], 0));
        window.observe(snapshot(&[(2, 2, 500.5, 1024)], 0));
        let report = window.report("idle");
        assert_eq!(report["matched_cpu_seconds"], 1.5);
        assert_eq!(report["new_identities"], 1);
        assert_eq!(report["departed_identities"], 2);
        assert_eq!(report["missing_observations"], 1);
        assert_eq!(report["counter_regressions"], 1);
        assert_eq!(report["resident_kib"]["sampled_peak"], 3);
        assert_eq!(report["resident_kib"]["last"], 1);
    }
}
