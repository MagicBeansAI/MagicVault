# Owned-child launch-stage investigation — 2026-09-08

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

Pending one manual, fail-fast run of the original concurrent process/HTTP
fixture: at most 200 fresh trials within ten minutes, no failed-trial replay.
