# Automatic browser connection qualification — 2026-09-07

Scope: standalone source 0.6.0, extension 0.5.0 and browser effect adapter 0.4.1.
This records local execution and reviewed boundaries, not a published package,
signed release, Chrome Web Store listing or production-safety certification.

## Automated evidence

macOS Apple Silicon; Rust compilation and release artifacts on an external SSD.
The Rust fixtures use private temporary roots, synthetic credentials and in-memory
or test-only keys. They never open a live vault, OS keychain or installed native
host. Commands below accept any writable artifact directory.

```bash
export CARGO_TARGET_DIR=/absolute/path/to/build-artifacts
export CARGO_BUILD_JOBS=4
cargo test --offline --locked -p magicvault-service -p magicvault-effect \
  -p magicvault -p magicvault-mcp --quiet
cargo build --offline --locked --release -p magicvault -p magicvault-mcp
make test-extension
make check
make test-architecture test-build-paths
make package-extension
```

| Lane | Result | What it covers |
| --- | --- | --- |
| Standalone Rust suites | 105 passed; 7 explicitly opt-in browser/public-web cases ignored | CLI/MCP, actual native host, daemon/control-state reload, browser adapters, new HTTP/process delivery, extension diagnostics and unchanged foundation boundaries |
| Extension and package JavaScript | 59 passed | Actual worker/options/fill sources, assembled assets, fixed public identity, backoff, pause, stale events, setup status, access/blocklist and package launcher |
| Architecture/build-routing tooling | 24 passed | Architecture drift detection and build-path behavior |
| Workspace all-target check | Passed | All workspace targets compile; durability guard retains its 7 reviewed baseline entries, no additions |
| Optimized binaries | Passed | CLI/daemon, native host and MCP built together |
| Native-host repeat run | All 3 tests passed on each of 5 additional runs | Real host process framing, synthetic daemon fill, closed errors and rejected-argument confidentiality |

The native connection regression cases cover independent profiles, rejection of a
live copied identity without eviction, quiet-host EOF, remembered reconnects that
do not take the human-prompt gate, durable denial and explicit reapproval, exact
extension binding at final registration, profile/client revocation, and a fresh
broker/store/registry reload with new handles but the existing approval.

Installation fixtures verify repeated exact registration, missing managed-file
repair, interrupted identity migration, lock contention, and refusal of foreign,
modified, symlinked or different-client definitions. They use synthetic browser
manifest paths, not the OS-wide live native-messaging directories.

The setup/doctor diagnostic fixtures additionally verify missing and complete
registration without creating installer locks, refusal of modified/symlinked or
unsafe files, corrupt pairing handling, and no repair side effects. An actual
local IPC fixture checks that doctor sends only authenticated status/browser-list
requests, counts extension profiles rather than CDP, and never prints pairing
tokens. Additional cases cover wrong-client guidance, closed errors, absent roots
without initialization, and a silent daemon's two-second extension-probe deadline.
No-connection results leave browser installation unconfirmed instead of claiming
the extension is absent. The 13 focused native-registration and setup/doctor tests
also passed separately before the complete standalone suite.

## Real Chrome smoke check

Headless Chrome 152; a disposable profile on the external SSD, no websites granted
and no native-host installation changed. Load the actual assembled `dist/extension`
using the browser-level debugging extension loader; open its options page.

- Worker and actual packaged `fill.js` registered successfully.
- Runtime extension ID matched the fixed bundled ID; both identity chips rendered.
- An unavailable/unauthorized-to-this-test native host produced a visible automatic
  retry deadline without manual Connect or any automatic website permission grant.
- Clicking **Pause connection** immediately showed paused status and removed both
  retry/handshake alarms.
- After closing/reopening Chrome, the debugging loader required loading the test
  extension again. Its local profile ID and paused state remained unchanged, with
  zero alarms. This is not evidence that this temporary debugging registration
  persists automatically like a normal installed extension.
- The disposable browser was closed. No pairing capabilities were printed or
  included in screenshots/results; only value-free status and public IDs were read.

## Review corrections and limits

An intermediate native liveness implementation used the effect mutex. An actual
host fill test exposed intermittent refusal under concurrent status polling.
The corrected implementation peeks through a separate owned socket descriptor,
and disconnect explicitly shuts down the shared socket. A regression case performs
200 exchanges while another thread executes 100,000 health probes; none can acquire
or contend with the delivery lock. This is concurrency evidence, not a latency or
throughput benchmark. The corrected standalone suites and repeated host runs passed.

The review also tightened final registration/revocation to the exact extension
and profile binding, and retained native installation's exact-file ownership checks.
Unknown/denied profiles persist a refusal before prompting, so interrupted consent
cannot become a repeated unattended prompt. Effects remain one-use and are never
replayed by the connection state machine.

The [architecture baseline](../architecture.md) records handshake v2, profile
capability storage, bounded persisted grants, automatic connection state and the
installer ownership boundary. Shared custody core, primitives, agent protocol and
MagicRun source/dependency revision are unchanged; Magician was not modified.

**Not executed:** real native approval/keychain dialogs, live LaunchAgent setup,
Chrome-to-OS-installed-host-to-daemon delivery in multiple real profiles, Store
identity/signing, and legacy installed-ID migration. The expanded
[installed-extension acceptance matrix](extension.md) remains the release gate
for those scenarios. Synthetic approval is a test seam, never a production option.

Chrome's documented [alarm scheduling](https://developer.chrome.com/docs/extensions/reference/api/alarms),
[native messaging identity restrictions](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging)
and [manifest public key](https://developer.chrome.com/docs/extensions/reference/manifest/key)
inform the implementation. Alarm deadlines are not exact wall-clock guarantees;
the manifest public key fixes unpacked identity, not publisher authenticity.
