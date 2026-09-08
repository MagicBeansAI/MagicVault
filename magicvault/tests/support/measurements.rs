//! Value-free counters for the opt-in synthetic qualification process only.
//! The broker shares this process; these are not standalone-daemon benchmarks.
use serde_json::{json, Value};

fn usage(who: libc::c_int) -> libc::rusage {
    let mut value = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: getrusage initializes this correctly sized writable structure.
    assert_eq!(unsafe { libc::getrusage(who, value.as_mut_ptr()) }, 0);
    unsafe { value.assume_init() }
}
fn cpu_us(value: &libc::rusage) -> u64 {
    [value.ru_utime, value.ru_stime]
        .iter()
        .map(|t| u64::try_from(t.tv_sec).unwrap() * 1_000_000 + u64::try_from(t.tv_usec).unwrap())
        .sum()
}
pub fn snapshot() -> Value {
    let own = usage(libc::RUSAGE_SELF);
    let children = usage(libc::RUSAGE_CHILDREN);
    #[cfg(target_os = "macos")]
    let (resident_kib, peak_kib) = {
        let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
        // SAFETY: the flavor selects precisely this buffer; sample this test
        // process, never enumerate or inspect a personal daemon/browser.
        assert_eq!(
            unsafe {
                libc::proc_pid_rusage(
                    std::process::id() as i32,
                    libc::RUSAGE_INFO_V2,
                    info.as_mut_ptr().cast::<libc::rusage_info_t>(),
                )
            },
            0
        );
        (
            unsafe { info.assume_init() }.ri_resident_size / 1024,
            own.ru_maxrss as u64 / 1024,
        )
    };
    #[cfg(target_os = "linux")]
    let (resident_kib, peak_kib) = {
        let statm = std::fs::read_to_string("/proc/self/statm").unwrap();
        let pages: u64 = statm.split_whitespace().nth(1).unwrap().parse().unwrap();
        let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        assert!(size > 0);
        (pages * size as u64 / 1024, own.ru_maxrss as u64)
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let (resident_kib, peak_kib): (Option<u64>, Option<u64>) = (None, None);
    json!({"self_cpu_us":cpu_us(&own), "reaped_children_cpu_us":cpu_us(&children),
        "resident_kib":resident_kib, "high_water_resident_kib":peak_kib})
}
