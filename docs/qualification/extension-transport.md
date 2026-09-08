# Repeatable native transport qualification

This opt-in conformance lane runs **real Chrome → extension → native host → daemon
→ shipped MCP**, with synthetic credentials and deterministic test-only consent.
It found a real compatibility bug that mocked Chrome APIs missed: current Chrome
returns 32-character hexadecimal document IDs, not hyphenated daemon UUID handles.

## Isolation and prerequisites

- macOS with a trusted Chrome executable supporting the debugging-only
  `Extensions.loadUnpacked` command. The recorded run used Chrome 152; this lane
  does not qualify every browser allowed by the production manifest's minimum.
- Rust/Node development prerequisites from the README. Builds use the normal
  `CARGO_TARGET_DIR`; browser profiles can use a separate explicit existing SSD directory.
- No personal profile, real credential, global native-host installation, keychain
  or LaunchAgent is used. The test owns and closes only its newly spawned browsers.

The fixture creates two **separate user-data directories**, not two named profiles
under a personal Chrome root. Each contains its own `NativeMessagingHosts` manifest
pointing to a private test wrapper/config. Chromium resolves this user-level
directory from its user-data root ([Chromium path implementation](https://github.com/chromium/chromium/blob/main/chrome/common/chrome_paths.cc)).
Normal production installation still uses OS-user definitions. The local debugger
and unsafe-extension-debugging launch flag belong solely to these disposable
test browsers; the production extension transport does not require CDP.

The copied fixture manifest **adds one required loopback host permission**
(`http://127.0.0.1/*`), while worker/options/fill code is unchanged. This pregrant
qualifies dispatch, not Chrome's real permission request or revocation UI. It
must never be added to the production manifest or treated as native acceptance.
The in-memory key provider and synthetic human live only in test executables.

## Commands

```bash
export CARGO_TARGET_DIR="$(make -s print-target-dir)"
export CARGO_BUILD_JOBS=4
export MAGICVAULT_CHROME='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
# Optional existing directory; each browser receives a new disposable child:
export MAGICVAULT_BROWSER_TMPDIR=/Volumes/SSD1/magicvault
make test-extension-native
make test-delivery-latency
make test-service-reliability
```

The browser lane builds matching release executables and runs one ignored test.
It verifies distinct profile identities, exact target discovery, 20 fills and one
denial, reactive input/no submit, navigation and site blocking during pending
consent, target omission after blocking, independent Pause/Resume, fresh handles
and remembered reconnect without another authorization prompt. Canary checks
cover tool receipts and audit; page evaluation returns only synthetic comparisons.
No raw capabilities, page dumps or credential values are printed.

The delivery lane performs 20 fresh CLI → daemon → MagicRun child operations and
20 CLI → daemon → loopback HTTP requests, in separate private roots to respect
retained-job capacity. The recipient checks the canary and deliberately echoes
it; CLI stdout/stderr and audit must withhold it. Every operation must complete.

Both lanes print bounded, value-free min/median/nearest-rank p95/max microseconds.
Timings include synthetic consent and status polling; CLI timings include process
startup. They exclude enrollment, destination registration and browser startup.
There is no brittle latency pass threshold and no claim of native-dialog latency,
Internet/TLS throughput, constant-time secrecy or long-soak stability.

`test-service-reliability` adds 32 process and 32 HTTP deliveries, refusal of the
next new operation at retained-job capacity, and reconciliation without duplicate
dispatch. It also exercises 2,000 paced status requests from four concurrent IPC
clients, the 16-connection admission cap, recovery after those connections close,
and shutdown with an unfinished frame. A separate case shuts down an actual
process tree and unfinished HTTP response, checking stopped work and no replay.
CPU/RSS counters cover the **in-process synthetic broker plus test driver**;
reaped-child CPU is reported separately. They do not measure a real keychain,
native human UI, installed standalone daemon or complete browser process tree.
The run is bounded to seconds, not a production soak or throughput guarantee.

## Qualify the actual npm-installed assets

```bash
# Keep builds/tarballs/browser profiles on SSD; parent must exist.
candidate_dir=$(mktemp -d /Volumes/SSD1/magicvault/candidate.XXXXXX)
make build-standalone
make package-npm NPM_SCOPE=@magicvault-local PACKAGE_OUTPUT="$candidate_dir/packages"
make test-package-browser PACKAGE_OUTPUT="$candidate_dir/packages" \
  PACKAGE_TEST_OUTPUT="$candidate_dir/qualification"
# This target explicitly places its disposable application under /private/tmp.

# Independent trials stop on the FIRST failure; they never retry failed work.
# Keep build/tarball/browser-profile artifacts on SSD1, but place the fresh test
# application on the internal temporary volume to avoid implicit removable-
# volume authorization. This is not permission/OS-policy acceptance.
node scripts/qualify-package.mjs --packages "$candidate_dir/packages" \
  --work "$candidate_dir/reliability" --app-parent /private/tmp \
  --with-rust-tests --with-browser-tests --reliability-rounds 10 \
  --with-performance-tests
```

This is an offline local tarball install, not registry publication. It checks
app-only setup, version/doctor, activation, actual installed CLI/MCP/native host,
and the same browser lane using copied installed extension assets (including the
explicit fixture-only permission change). npm removal must leave stable native
executables usable; application uninstall archives only the test application and
does not create a vault or touch a keychain/service. Test-driver Rust is required;
the installed launchers themselves do not compile Rust.

`--reliability-rounds` accepts 1–20; multiple rounds and performance tests require
`--with-rust-tests`. Browser qualification requires explicit `--app-parent`;
normally use `/private/tmp` on the internal volume. It must be an existing absolute directory; the
driver creates a new private child and reports its artifact location. It never
reuses a live app. A failed run leaves its isolated app for diagnosis; a successful
run performs recoverable app removal and keeps the archive as evidence.

### Browser process resources

For one bounded resource trial, add `--browser-idle-secs 120` to a fresh
`--with-rust-tests --with-browser-tests --app-parent /private/tmp` invocation.
The flag accepts 30–300 seconds, requires one round, and refuses simultaneous
stack sampling. Default qualification has no extra idle delay. Direct Rust test
invocation uses the test-only `MAGICVAULT_BROWSER_RESOURCE_IDLE_SECS` variable;
the package driver clears ambient values and sets it only from the explicit flag.

This samples the two disposable browsers after setup and after each of 21
fill/deny operations, followed by an approximately one-second cadence during
the idle window. It asserts both profile handles remain connected, with no
additional native-host launches or synthetic confirmations during idle.
No personal process enumeration, command lines, environments or memory contents
are read. The peer's browser PID must match the fixture-owned child.

CPU comes from [CDP `SystemInfo.getProcessInfo`](https://chromedevtools.github.io/devtools-protocol/tot/SystemInfo/#type-ProcessInfo)
in cumulative seconds. Current resident and physical-footprint bytes come from
macOS `proc_pid_rusage`, not a lifetime peak. CPU deltas match PID **and process
start identity** between adjacent samples. Missing, new, departed or regressed
counters are reported; short-lived unsampled work is not included. Memory sums
can count shared pages more than once. Reported peaks are sampled peaks only.
The population excludes native hosts, MCP, the test broker and a standalone
daemon. These measurements include CDP observer overhead and the fixture's 65
unrelated tabs; they are not MagicVault-only overhead or latency guarantees.
Do not interpret a two-minute idle observation as a long soak.

An external-volume candidate has reproduced a native-host stall in macOS `dyld`
before MagicVault's entry point. New fixture-scoped stack and policy observations
associate that path with removable-volume authorization; it is not qualified for
unattended startup. Internal-volume comparisons passed with identical host bytes.
See the [policy investigation](results-startup-policy-2026-09-08.md) and retained
[original failures](results-reliability-2026-09-08.md). Do not grant Full
Disk Access, disable Gatekeeper, approve native prompts automatically or increase
the acceptance deadline to make this lane green. After missing its 15-second
connection bound, the browser fixture observes up to 45 seconds for closed stage
diagnostics; even a late connection remains a failure. No fill follows that failure.

For a deliberately authorized slow-start diagnostic, set
`MAGICVAULT_TEST_STARTUP_DIAGNOSTICS` to an existing absolute private directory
(mode `0700`). After two seconds, the test can sample only its newly launched
host, checking the recorded PID's executable against the expected installed host.
A bounded one-second stack sample stays in a private child directory; public
output contains only stages and capture success. This never reads frames,
capabilities or process environments. It does not alter OS access policy.
Sampling perturbs timing: keep it disabled in the normal performance lane, and
never publish raw traces. A fixture using an external application may trigger an
OS permission request despite its synthetic MagicVault consent provider.

`MAGICVAULT_TEST_CLI`, `MAGICVAULT_TEST_MCP`, `MAGICVAULT_TEST_NATIVE_HOST` and
`MAGICVAULT_TEST_EXTENSION` are test-only seams, not production configuration.
Do not point them at unrelated or untrusted binaries/assets. Default tests and
the unsigned candidate workflow do not implicitly launch a browser.

## What still needs a person or separate release authorization

Use the [installed-extension matrix](extension.md) for genuine Chrome permission
approval/removal, native prompt readability and denial, keychain behavior, browser
restart/long outage, production host definitions and full installation recovery.
Use [native distribution acceptance](distribution.md) for LaunchAgent/upgrade,
quarantined downloads and signer verification. A real Codex/Claude model session
is a separate client acceptance gate: official SDK stdio conformance does not
prove agent tool-selection quality. MCP setup instructions use the official
[Codex](https://developers.openai.com/codex/mcp) and
[Claude Code](https://code.claude.com/docs/en/mcp) interfaces; no live account or
agent configuration is changed by these tests.
