# Native startup policy and process follow-up — 2026-09-08

Status: **external-volume startup limitation identified; process uncertainty
still open**. This is not a production fix or completion of the public release gate.

Qualification changes accompany this record, based on `fb24d8f`. The tested
installed executables are the unchanged unsigned `0.8.2` CI artifact from
`23e648ccfbed191bc4090955b04a0e73552dd99a`, [run 34179148138](https://github.com/MagicBeansAI/MagicVault/actions/runs/34179148138).
Its independently verified tarball hashes are recorded in the
[completion-order results](results-completion-order-2026-09-08.md).
The new test-driver/workflow changes have not yet been run in CI.

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
No full Rust workspace rerun or new remote CI run is claimed for this diagnostic-only
change; the touched Rust test targets compiled and the explicit probes ran.

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
