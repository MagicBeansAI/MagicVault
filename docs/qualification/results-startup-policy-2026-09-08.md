# Native startup policy and process follow-up — 2026-09-08

Status: **external-volume startup limitation identified; process uncertainty
still open**. This is not a production fix or completion of the public release gate.

Qualification changes accompany this record, based on `fb24d8f`. The tested
installed executables are the unchanged unsigned `0.8.2` CI artifact from
`23e648ccfbed191bc4090955b04a0e73552dd99a`, [run 34179148138](https://github.com/MagicBeansAI/MagicVault/actions/runs/34179148138).
Its independently verified tarball hashes are recorded in the
[completion-order results](results-completion-order-2026-09-08.md).
The test-driver/workflow changes are committed as
`b475f3c32090b018d7f0378f1c5aa4d13d5dfeec`; their first CI attempt passed as
recorded below. The earlier local comparisons still refer to the `23e648c` artifact.

Local environment: macOS 26.6 arm64, Rust 1.92.0, Node 22.19.0 and Chrome
152.0.7977.82. SSD build/package/profile storage is unchanged. Each trial owns
fresh synthetic custody, recipients and browser user-data directories. No live
vault, managed service, personal browser profile or keychain was used or upgraded.
Automation did not click or submit native OS authorization. Raw stack/policy
records remain private; the external-path trials are diagnostics, not proof that
the workflow needs no OS interaction.

## Native host: new evidence

| Trial using identical CI bytes | Observation |
| --- | --- |
| Fresh external application, no sampler | PASS; profile starts 6.880 s / 0.428 s |
| Another fresh external application, early private sampler | PASS; 3.386 s / 0.336 s; first profile was sampled before host entry |
| Fresh internal temporary application, same instrumented driver | PASS; 0.835 s / 0.447 s; no sampling threshold reached |
| Final corrected Make target, internal application, sampling disabled | PASS; 0.813 s / 0.352 s |

Each trial also passed all 13 installed CLI/process/HTTP/native-host/MCP cases
and the real two-profile extension case, including 20 measured fills, denial,
navigation/site-block refusal and pause/reconnect. Successful cleanup removed
only its npm installation and recoverably archived its application. These are
small observations, not a latency guarantee or replacement for the earlier failures.

In the sampled trial, unpacked extension loading finished at 179 ms, its options
page at 312 ms and fixture page at 357 ms. A one-second, fixture-PID-verified sample
then showed all 89 observations in the single main thread's
`dyld` → `getOnDiskBinarySliceOffset` → `mapFileReadOnly` → `__open` stack, with
176 KiB footprint, before MagicVault initialization. No native frame or
credential value was collected.

The same PID's policy requests show a quick Full Disk Access **preflight denial**,
then a non-preflight `SystemPolicyRemovableVolumes` request and an OS prompting
event. That request returned about 2.60 seconds later, just before startup settled.
This supports removable-volume authorization as the source of this sampled delay,
not a MagicVault handshake deadlock or a need for Full Disk Access. It does not
establish who supplied the OS decision or justify automatic approval.

Retained logs for the **original** sampled 45-second failure also name its exact
host in the removable-volume authorization path, with the response appearing
about 86 seconds after launch. They associate the original external-volume
failure with the same access-policy path, but do not resolve every part of that
long wait. Historical unsampled startup failures must not all be assigned this cause.

Apple's published [launch-loader implementation](https://github.com/apple-oss-distributions/dyld/blob/main/dyld/JustInTimeLoader.cpp)
passes the main executable path into the slice-offset lookup;
the [lookup implementation](https://github.com/apple-oss-distributions/dyld/blob/main/dyld/Loader.cpp)
reopens/maps that file. This is consistent with the captured stack. These are
upstream source references, not an exact-source attestation of the installed OS.

## Boundary and qualification correction

Keep the runtime application on the internal user volume: normal setup already
defaults to `~/.magicvault-app`. External/removable application installations
are **not qualified for unattended native-host startup**. Keeping Cargo output
and tarballs on SSD1 does not require running the installed host from that volume.
No launcher rewrite, symlink workaround, entitlement, timeout relaxation, Full
Disk Access grant, Gatekeeper change or OS authorization bypass is introduced.

The packaged browser driver now refuses an omitted `--app-parent` before creating
artifacts. Its Make target explicitly chooses `/private/tmp` for a fresh private
application; an explicit external parent remains available for deliberate policy
diagnosis. This is a test-harness correction, not a blanket external-filesystem
ban in the production installer. [Runbook](extension-transport.md).

The normal 15-second connection acceptance bound is unchanged. Optional early
sampling starts after two seconds, requires a new fixture wrapper launch and
matching executable identity, retains only a private bounded stack file, and is
disabled by default. Instrumented measurements must be labelled diagnostic.

## Process uncertainty: not resolved

Static tracing of the original `uncertain` / `unavailable` / `may_have_run: true`
receipt narrows it to a dispatched, unsuccessful settled terminal. It is not the
adapter timeout, transport-error or audit-persistence branch. Candidate causes
such as a nonzero recipient exit, signal termination or resource terminal cannot
be distinguished from that public receipt alone. A missing marker does not prove
that dispatch never occurred. The separate fixed admission race is not its cause.

The new opt-in probe runs the governed executor on Tokio's blocking pool while
an independent Tokio child owner produces exit notifications. It retains the
original two-second process limit, 200-delivery workload and 90-second harness
budget; alternate trials use the original minimal recipient script so extra
stage writes are not assumed timing-neutral. Only closed terminal/known-error
booleans exist under `cfg(test)`; no streams or arbitrary exit codes reach agents.
The final form distributes ten noise children across each delivery trial and
awaits both owners before propagating a governed-worker assertion failure.

Both final probes passed locally: 200 async deliveries with 2,000 distributed
noise children in 32.834 seconds, then 200 serial deliveries in 34.155 seconds.
The original CI failure was **not reproduced**. The packaging guard's 12 Node
tests also passed, including refusal before artifact creation when browser
application placement is implicit.

Final local regression verification: architecture baseline matches all 71 inputs;
24 architecture/build-routing tests and all 86 extension/distribution/fixture
JavaScript tests passed. The loopback fixture initially received sandbox `EPERM`
before listening; rerunning that unchanged five-test suite with loopback permission
passed. The corrected Make target passed the installed client/browser case above.
That local pass did not rerun the full Rust workspace; the touched Rust test
targets compiled and the explicit probes ran. The subsequent CI pass below
includes a complete workspace test run.

Run both explicit probes, without retrying a failed delivery:

```bash
export CARGO_TARGET_DIR="$(make -s print-target-dir)"
cargo test --locked -p magicvault-effect --lib process::reliability:: \
  -- --ignored --nocapture --test-threads=1
```

Unsigned distribution CI is configured to run both probes before its existing
20 fail-fast installed-client trials. Until the original CI failure is captured
with a terminal diagnosis and corrected (or an explicit supported limitation is
established), **this release blocker remains open**. Local passing stress trials
are not closure. Core, MagicRun, Magician, production extension/service code,
component versions and agent/native wire formats are unchanged.

## macOS 15 CI follow-up

[Unsigned distribution run #9](https://github.com/MagicBeansAI/MagicVault/actions/runs/34182556083)
completed **successfully on attempt 1**, on the exact diagnostic commit
`b475f3c32090b018d7f0378f1c5aa4d13d5dfeec`. It ran on macOS 15.7.9 arm64
(runner image `macos-15-arm64/20260829.0321`) with Rust 1.92.0 and Node 22.23.2.
No failed job or delivery was retried.

| Lane | Result |
| --- | --- |
| All-target compilation and standalone release build | PASS |
| Default Rust workspace suite | PASS; 254 tests, 13 opt-in cases ignored |
| JavaScript extension/distribution/fixture suites | PASS; 86 tests |
| Architecture/build-routing suites and baseline | PASS; 24 tests, 71 fingerprinted inputs |
| Serial governed-process probe | PASS; 200 deliveries in 1.330 s |
| Async governed-process probe | PASS; 200 deliveries plus 2,000 noise-child exits in 7.066 s |
| Installed CLI/process/HTTP/native-host/MCP trials | PASS; all 20 rounds, 260 test executions |
| Explicit delivery/resource/capacity/shutdown cases | PASS; all three cases |
| Offline npm removal, stable executables and recoverable application retirement | PASS |
| Unsigned candidate artifact upload | PASS |

Artifact `10039541723`, `MagicVault-UNSIGNED-darwin-arm64`, contains the two
candidate tarballs; the Actions ZIP is 6,530,093 bytes. Its reported SHA-256 is
`d51c47a7d72734ae256419b791241e5544470d8bd57c27a74b1ff081e53fe750`.
The artifact expires on 2026-09-22. This follow-up verified workflow logs and
artifact metadata, not an independent download or browser installation of these
newly produced bytes. The workflow did not run real browsers, genuine native
consent/keychain acceptance, signing, notarization or publication.

**The original intermittent process failure did not reproduce.** This establishes
that the new diagnostics compile and pass in the originally affected runner
environment; it does not identify or fix that failure. The open release finding
and the external-volume startup limitation above remain in force.
