# Owned-child launch-stage investigation — 2026-09-08

Follow-up: the [native-spawn correction](results-native-spawn-2026-09-08.md)
removes this userspace fork interval and records its first-attempt 200-trial
result. The failed-run evidence below remains unchanged.

The [preceding CI recurrence](results-os-exit-reason-2026-09-08.md#named-namespace-follow-up)
identified `Observed(Foundation)` on an owned child killed before cleanup, with
all recipient markers absent. That OS namespace does not identify a framework
defect, exception site or whether exec occurred. This follow-up localizes the
existing pre-exec callback boundary, not a speculative production fix.

## Scope and interpretation

This follow-up builds on MagicVault `f4c53dc` and selects MagicRun
`db545df65dcd1bf6b62f79111c5693da45f62bff`. Package/crate versions are unchanged.

Only an active debug diagnostic capture on macOS allocates one anonymous shared
mapping containing an atomic byte. The parent initializes and owns the mapping;
the existing child callback stores entry and successful completion only. No
child allocation, lock, TLS, logging or extra descriptor is added. The parent
reads a closed stage after spawn returns, even on error. Allocation failure is
`Unavailable`; unexpected byte values become `Invalid`. Neither affects delivery.
The Command retains ownership through spawn; the final parent owner unmaps it.
Exec/exit discards the child's mapping. Capture capacity remains bounded at 16.

| Observation | Meaning, not a stronger claim |
| --- | --- |
| `CallbackNotEntered` | No first callback store observed; does not locate the preceding exception |
| `CallbackEntered` | First store observed, but successful callback completion not observed |
| `CallbackCompleted` | Callback's final store observed; exec or recipient entry may still fail |
| `Unavailable` / `Invalid` | No usable stage classification; never permission to retry |

Rust 1.92's [Unix process source](https://github.com/rust-lang/rust/blob/1.92.0/library/std/src/sys/process/unix/unix.rs)
returns a child handle on EOF from the child error pipe, which can also follow
death before exec. The existing registered callback already selects the fork
path. The observer does not add a launch-mode-changing callback. Synthetic
children explicitly test death before/inside/after the callback, a callback
error, successful exec and failed exec after callback completion.

Normal builds omit all hooks and standard release builds reject the custom cfg.
Only the synthetic broker/test driver is instrumented, not packaged clients.
No PID, address, raw crash code/payload, path, argument, environment or credential
is retained. No broad process inspection, OS-log collection, live keychain or
browser change, retry, deadline relaxation, concurrency reduction, signing or
publication is part of this work. Magician remains untouched; its future upgrade
must review the changed literal governed-source fingerprint normally.

## Local validation

macOS `26.6` arm64, Rust `1.92.0`, separate normal/diagnostic outputs on SSD1:

- Nine MagicRun diagnostic units passed, including all six real launch boundary
  cases, unavailable/invalid classification, no allocation without capture,
  parent mapping ownership, existing OS decoder coverage and capture bounds.
- Both real batch diagnostic tests passed: normal/nonzero/self-signal recipients
  and explicit deadline cleanup, now asserting callback completion too.
- Normal MagicRun all-target check, 17 batch tests and eight PTY tests passed.
  Its reviewed 50-input architecture baseline and 11 gate tests passed.
- MagicVault's all-target check, five normal process tests and eight HTTP tests
  passed with only the intended dependency revision changed in Cargo.lock.
- All five real adapter diagnostic cases passed with callback completion and
  matching signal/OS reason categories. The exact CLI fixture compiled with the
  debug-only stage re-export; execution is reserved for the bounded CI run.
- 18 packaging/orchestration tests, 12 architecture tests and the reviewed
  74-input architecture baseline passed.

The earlier failures, including the local diagnostic timeout, remain retained;
passing this probe's tests is not a reliability fix. No local test failed in this
follow-up. The CI run below must supply the next original-scenario observation.

## Bounded CI evidence

[Run 34194598193, attempt 1](https://github.com/MagicBeansAI/MagicVault/actions/runs/34194598193)
**FAILED** on MagicVault `44f5a05d74518e36f0a35896929433db0976d0da`, using
MagicRun `db545df65dcd1bf6b62f79111c5693da45f62bff`. Job `101959478612`;
macOS `15.7.9` arm64 (`24G830`), image `20260829.0321.1`, Rust `1.92.0`,
Node `22.23.2`. The reviewed client build/package assembly, orchestration and
architecture checks, three diagnostic registry units, five real adapter cases,
exact CLI fixture compilation and explicit instrumented-release refusal passed.

Trials 1–21 passed both original cases and reported `CallbackCompleted` for the
process. **Trial 22 failed the process case, while its HTTP companion passed.**
The loop ran from `06:30:09.933Z` to failure at about `06:30:29.359Z`—roughly
19 seconds, well before the ten-minute limit. No trial 23, retry, successful
summary, post-success uninstall or upload followed. Runner teardown, not any
live installation, owns the disposable failed fixture.

| Failed-child observation | Result |
| --- | --- |
| Launch stage | **`CallbackNotEntered`**: mapping available, no first callback store observed |
| Spawn and first wait | One child handle; matching owned child / `SIGCHLD`, `CLD_KILLED`, `SIGKILL` on first wait; no wait interruption/error |
| OS exit reason | **`Observed(Foundation)`**, before cleanup |
| Cleanup and final reap | No explicit termination cleanup; later before-reap group kill returned `NoSuchProcess`; final reap was `SIGKILL` with no wait error |
| Adapter and markers | Dispatched `RuntimeFailure`, `uncertain` / `unavailable`, `may_have_run: true`; stderr empty; entry/material-present/completion markers all absent |

## Interpretation and next boundary

This occurrence is now localized **before the callback's first observed store**,
not inside its later working-directory/resource-limit operations or after its
successful completion. A child handle was returned without a normal recipient
launch, consistent with Rust's error-pipe EOF behavior. Cleanup still follows
the already-observed signal exit; it does not explain the original kill.

The remaining launch interval includes macOS's fork/at-fork child work and Rust's
pre-callback setup (standard descriptors, process group and signal setup). The
probe does not distinguish those operations, identify a specific exception,
framework or signal sender, or exclude an early failure at the marker boundary
itself. `Foundation` remains an OS category, not attribution to a Foundation API.
No program counter, stack, Mach exception payload or broad OS history was read.

The next useful work is a focused review of that pre-callback launch interval and
a regression-backed fix preserving exact executable/cwd authority, clean
environment, descriptor handling, owned-group cleanup and original process/HTTP
concurrency. If further evidence is required, scope it to the owned synthetic
child rather than collecting general crash reports. Do not disable OS fork
safety, serialize away the scenario, relax deadlines or replay uncertain work.

This completes the launch-stage diagnostic step, **not the process reliability
fix or release qualification**. All earlier failures remain retained. No further
run, production launch-policy change, Magician upgrade, signing or publication
followed this result.
