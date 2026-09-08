# Changelog

## Unreleased — Qualification diagnostics

- Record the [focused CI failure and independent candidate qualification](docs/qualification/results-focused-candidate-2026-09-08.md):
  trial 42 observed the owned child already `SIGKILL`-terminated before cleanup;
  sender/OS reason remains unknown and the first failure is retained. Downloaded
  `01a1cfd` package hashes, installed CLI/MCP, real CDP and two-profile extension
  tests passed separately. No process runtime fix or release readiness is claimed.
- Add a separate manual bounded process investigation: fresh exact-path trials,
  original HTTP companion concurrency, fail-fast behavior and a ten-minute trial
  budget. Passing trials explicitly remain inconclusive about the prior failure.
- Correct packaged-browser qualification to select the downloaded CLI for its
  real-CDP fixture and run that case alongside installed native-extension/MCP
  coverage. This changes test orchestration only; shipped binaries, shared core,
  MagicRun and Magician are unchanged by this follow-up.
- Extend the synthetic exact-process observer into MagicRun's owned-child wait,
  signal and cleanup path. Update only the locked MagicRun Git revision to
  `25f1c449`; standard builds omit all observer hooks and release builds reject
  the diagnostic cfg. No runtime fix is claimed. MagicRun source attestation
  bytes change; Magician's existing dependency and shared custody are untouched.
  See the [signal investigation](docs/qualification/results-process-signal-2026-09-08.md).
- Record first-attempt unsigned CI success on `01a1cfd`: normal conformance,
  20 installed-client trials and bounded capacity/in-flight shutdown tests pass.
  The original process failure did not recur; its cause remains an open gate.
- Require explicit application placement for packaged browser qualification;
  its Make target uses an internal temporary application while SSD build/package
  paths remain unchanged. Document the observed pre-entry removable-volume
  authorization delay, without bypassing OS permissions or claiming it fixed.
- Add opt-in private native-host startup sampling and an async process/child-exit
  stress probe. Keep the original process uncertainty open; no default runtime,
  extension, core, MagicRun, Magician or wire-format behavior changes. See the
  [investigation](docs/qualification/results-startup-policy-2026-09-08.md).
- Add an opt-in, bounded debug-only observer to the exact installed CLI →
  synthetic broker → MagicRun fixture. It captures closed categories only,
  validates real recipient failure classes, and is rejected by standard release
  compilation. Unsigned candidate CI runs 20 fail-fast diagnostic trials;
  passing repeats do not resolve the retained intermittent process failure.
- Add opt-in, bounded real-browser process CPU/resident/footprint observations
  and profile-connection idle assertions. Sampling uses only disposable browser
  peers, tracks PID start identity, and reports population gaps; it does not
  measure a live daemon or claim long-soak stability. Default runtime behavior
  and shared core/MagicRun/Magician remain unchanged.
- Record the [exact-path investigation](docs/qualification/results-exact-path-2026-09-08.md):
  CI reproduced a dispatched process `RuntimeFailure` in round 15 with no normal
  exit code or entry marker. Its signal/source remains unresolved; no retry or
  new artifact upload followed. The independent real-browser/resource trial
  passed on the prior available unsigned candidate, not the failed run's bytes.

## 0.8.2 — 2026-09-08 — Completion and admission ordering

- Fix a standalone scheduling race: browser, process/HTTP and metadata completion
  now release their human-operation slot in the final serialized transaction,
  after cleanup/audit and before readers can observe the terminal decision.
  An immediate next operation no longer races the previous async worker's wake-up.
- Explicitly drain background job futures/destructors during shutdown so early
  admission release cannot leave a finishing worker holding the instance lock
  across an immediate restart. Completed tasks are not retained as an unbounded list.
- Add deterministic current-thread regressions for successful and audit-uncertain
  fills, denied delivery and decided metadata. The browser regression was observed
  failing before the fix, then passing afterwards. See the [qualification record](docs/qualification/results-completion-order-2026-09-08.md).
- CLI/MCP/service `0.8.2`; core, primitives, MagicRun, wire formats, extension and
  vault schema are unchanged. This source version is not a publication or a claim
  that the separate intermittent process/native-startup findings are resolved.

- Preserve the reproduced installed-process CI failure as an open release gate;
  add fixture-only launch stages and a bounded synthetic process stress probe.
  The additional observations are diagnostics, not a claimed runtime fix.
- Add fail-fast installed-package repetition, explicit internal-volume temporary
  application placement, and bounded CPU/RSS, delivery/IPC capacity, recovery and
  in-flight process-tree/HTTP shutdown qualification. Preserve the browser's
  15-second pass threshold while observing failed startup for up to 45 seconds
  with value-free fixture stages. These are test/qualification changes, not a
  runtime fix; shared custody, MagicRun and Magician are unchanged.
- Expand unsigned candidate CI to the complete extension/fixture suites, 20
  independent installed-client trials and resource/shutdown probes; align the
  macOS/Ubuntu manual lane with the candidate toolchain and bounded workloads.
- Use “Let agents use credentials without seeing them” in the README and npm
  package copy, with the recipient/browser-observation boundary stated nearby.
- Shorten the README around the credential-use workflow, candidate installation,
  MCP quick start and compact surface coverage. Move full coverage/CLI recipes
  into linked guides, retain existing entry-point anchors, and separate packaged
  extension instructions from source builds without hiding native acceptance gaps.
- Add closed, value-free process-test failure diagnostics and record
  [unsigned `0.8.1` distribution CI](docs/qualification/results-distribution-ci-2026-09-08.md).
  The fresh workflow passed; its preceding intermittent process-delivery failure
  remains unresolved. No runtime fix, signing or publication is implied.

## 0.8.1 — 2026-09-08 — Native cancellation receipts

- Correct pending process/HTTP cancellation receipts when the native prompt
  provider returns `denied` while being torn down: report `cancelled`, consistent
  with browser fills. A genuine human denial remains `denied`; an explicit
  expiry remains `expired`. No recipient is dispatched by this correction.
- Add process/HTTP regression cases for explicit cancellation, consent reset,
  genuine denial and expiry precedence. Native acceptance found the discrepancy;
  the original refusal remained fail-closed with no delivery or saved grant.
- CLI/MCP/service `0.8.1`; protocol/effect `0.6.0`, agent wire `4`, extension
  `0.6.1` and shared core/primitives/MagicRun remain unchanged. No Magician update
  or vault migration is required. See [qualification](docs/qualification/results-cancellation-2026-09-08.md).
- Record [installed permission and in-flight recovery acceptance](docs/qualification/results-permission-recovery-2026-09-08.md):
  remembered browser use cannot bypass removed site access; restored access and
  fresh reconnect handles work; actual process/HTTP cancellation stops owned
  work while preserving post-dispatch uncertainty. One-host results do not close
  wider recovery, startup-reliability or signed/public-release gates.

## 0.8.0 — 2026-09-08 — Exact-use consent source alpha

- Add native **Allow once** and **Always allow** credential-use choices, with
  Deny as the default. Pairing, enrollment and registration remain separate.
  Models cannot submit allow decisions or arbitrary grant scopes.
- Remember exact paired-client/destination-profile uses, or exact browser
  identity/origin/frame-kind/credential/selector mappings. Native extension
  grants survive reconnect/restart; live CDP grants do not. Existing site,
  document, executable digest, cancellation and no-retry boundaries remain.
- Add human CLI `list-consents`, `revoke-consent` and `clear-consents`. Durable
  revocation cancels affected work and invalidates late native decisions.
  Bound grants to 16 records of 12 KiB, with a 1.25 MiB registry and unchanged
  reply limit. Add exact-scope, persistence, cancellation and failure coverage.
- CLI/MCP/service `0.8.0`, protocol/effect `0.6.0`, agent wire `4`; update the
  standalone bundle together. Extension `0.6.1` clarifies consent wording while
  retaining its bridge/profile handshake. Shared core/primitives, vault/key
  identity, MagicRun and Magician remain unchanged. Prior `0.7.0` results do not
  qualify this new consent policy; native acceptance is a separate gate.
- Document [consent scope, revocation and compatibility](docs/consent.md).
  No signing, publication or production-security certification is implied.
- Record [local consent qualification](docs/qualification/results-consent-2026-09-08.md),
  including unresolved native-extension startup timeouts and value-free fixture
  diagnostics. Passing synthetic consent tests does not qualify native selections.

## 0.7.0 — 2026-09-08 — Narrowed discovery source alpha

- Add optional exact `top_origin` and backend `tab_id` narrowing to MCP
  `browser_targets` and CLI `browser-targets`. Both must match when supplied.
  Extension and CDP adapters filter before bounded frame inspection and recheck
  actual document results. Filters are not grants or credential-use authority.
- Retain explicit `capacity` for oversized matching sets. A clean local CDP
  discovery overflow keeps the connection usable for a narrower query; transport
  failures still invalidate it. No silent truncation, grant changes or tab closure.
- Extend protocol/native-bridge, broker, Chrome API and real CLI/MCP/CDP/extension
  regression coverage. Existing unfiltered JSON requests and fill semantics remain
  unchanged; filtered discovery requires matching updated standalone components.
- Qualify basic installed macOS/Chrome discovery and native deny/allow delivery
  with a synthetic credential and real keychain/site/destination approval. Record
  upgrade timeout reconciliation and remaining lifecycle/release gates separately.
- CLI/MCP/service `0.7.0`, protocol/effect `0.5.0`, extension `0.6.0`. Minor crate
  versions identify new public discovery request/bridge types; shared custody
  core/primitives, vault/key identities, MagicRun and Magician remain unchanged.
  No signing or publication occurred. See [qualification](docs/qualification/results-discovery-2026-09-08.md).

## 0.6.2 — 2026-09-08 — Large-profile discovery source alpha

- Fix extension discovery failing for profiles with more than 64 total tabs.
  Query granted sites and skip discarded, blocked, unsupported and inaccessible
  tabs before applying a 128-candidate frame-inspection budget. Retain the
  128-target response limit and fail explicitly on eligible-set overflow.
- Cache permission checks only within one discovery revision; reject observed
  permission/policy changes and recheck actual frame/document origins. Fills
  retain fresh permission checks, exact document binding and human consent.
- Add large-profile, budget, navigation and permission-race regression cases;
  exercise real extension/MCP fills with 65 unrelated disposable browser tabs.
- CLI/MCP `0.6.2` package extension `0.5.2` as a new immutable bundle. Shared
  core, service/effect implementation, wire formats, MagicRun and Magician are
  unchanged. This remains an unsigned local candidate, not a public release.
- Document Chrome retaining an old resolved unpacked-extension path after app
  upgrade; verify the extension version and load updated assets without first
  removing the existing fixed-ID extension. See the
  [discovery qualification record](docs/qualification/results-discovery-2026-09-08.md).

## 0.6.1 — 2026-09-07 — Browser document compatibility source alpha

- Fix a real Chromium extension discovery/fill failure: browser document IDs can
  be 32-character hexadecimal tokens rather than hyphenated UUIDs. Preserve exact
  document identity and retain origin, lifecycle, permission and consent checks.
  Malformed, stale and differently cased tokens still fail closed.
- Add opt-in real Chrome/native-host/daemon/MCP qualification with disposable
  user-data roots, two profiles, denial, pending-consent navigation/blocks,
  repeated fills and independent pause/reconnect. Extend offline npm qualification
  to exercise the actual installed executables and extension assets.
- Add release-build CLI/process/loopback HTTP latency observations and canary
  checks, SSD browser-profile routing, repeatable technical runbooks and updated
  public evidence. Synthetic consent and a loopback permission fixture do not
  qualify genuine native permission/keychain or signed-download acceptance.
- CLI/MCP advance to `0.6.1` to package corrected extension `0.5.1` in a new
  immutable application bundle. Service `0.6.0`, effect `0.4.1`, shared custody
  core/primitives, wire formats, MagicRun and Magician are unchanged.
- See [executed qualification and remaining gates](docs/qualification/results-native-transport-2026-09-07.md).
  This is not a registry publication, signing/notarization or production release.

- Make the README MCP-first, with Codex/Claude Code connection commands and an
  agent-led quick start, followed by CLI automation and developer integrations.
  Clarify one-time human setup, per-use consent, local-only MCP and SDK availability.

## 0.6.0 — 2026-09-07 — Automatic browser connections source alpha

- Normal packaged setup installs the native host for a fixed public unpacked
  extension identity. ID copying is no longer needed. Exact managed definitions
  are idempotently repaired; foreign/modified files remain protected. Install-only
  remains custody/service/registration-free; upgrade preserves custom IDs.
- Extension `0.5.0` automatically connects with durable Chrome-alarm backoff,
  distinct per-profile capabilities, live status and persistent Pause. Denial,
  revocation, incompatible versions and duplicate-profile conflicts stop retries.
- Add advisory extension checks to normal setup and `doctor`: verify native-host
  definitions, count caller-owned extension connections, and give recovery steps.
  No connection means installation unconfirmed, not absent. Probes are bounded,
  never scan Chrome profiles or change permissions, and do not block other surfaces.
- Remember native discovery authorization per client/extension/browser profile;
  independent profiles no longer replace each other. Disconnect revokes that
  profile's remembered grant; client revocation clears all its grants. Every fill
  still requires credential policy and human consent; effects are never replayed.
- Detect idle native-host EOF and cancel stale pending effects. Add coordinated
  handshake v2 with closed refusal codes. CLI/MCP/service advance to `0.6.0`,
  effect to `0.4.1`; agent wire 3, effect/config schemas 1, core, primitives,
  MagicRun and Magician are unchanged.
- Document the one-time path-derived to fixed-ID migration, profile capability
  storage and public-key limitations. This is not signing, registry publication
  or a Store listing. See [connection qualification](docs/qualification/results-extension-connections-2026-09-07.md).

- Highlight the installed extension ID in a selectable, keyboard-focusable chip
  with light/dark styling, and show the profile ID used in native consent.

- Make browser access visible in a persistent status banner and disable/relabel
  the all-HTTPS button after approval. Refresh from Chrome on page return and
  permission changes; keep browser-grant status independent of blocklist errors.
  Add regression cases for reopening, denial, revocation and stale reads.

- Include the earlier unreleased site-access changes: one-time all-HTTPS or selected-site grants, with
  local HTTP separately opt-in. Add a persistent human-managed site blocklist
  enforced for discovery, top/frame targets and pre-dispatch fills. Keep existing
  exact credential-origin rules and per-fill consent. Fixed-ID upgrades preserve grants.
- Add trusted-context-only browser storage for bounded site settings (never
  enrolled credentials), closed failure handling, concurrent-write protection and access
  change/preflight regression coverage. Clearing HTTPS access retains local HTTP
  and blocks; the blocklist is not Chrome permission revocation or effect recall.
- Add setup UI tests and actual assembled-worker/import startup coverage, including
  missing `fill.js` rejection; document unpacked-directory selection and reloads.
  Shared core, MagicRun and Magician remain unchanged.

- Fix distribution workflow validation by exporting runner-dependent artifact
  paths from the first execution step, not unsupported job-level expressions.
  Add a focused runner-path/export regression test; application behavior is unchanged.

## 0.5.0 — 2026-09-07 — Prebuilt distribution source alpha

- Add explicit-scope macOS Apple Silicon npm package assembly, exact-version thin
  CLI/MCP launchers and an unsigned candidate workflow. No npm publication or
  Apple signing/notarization is performed automatically.
- Add native `setup`, `doctor`, `upgrade` and recoverable `uninstall`, with private
  immutable application bundles separate from custody, integrity validation,
  installer locking, stable service/MCP/extension paths and writer-drain gating.
- Let packaged extension setup find its stable native host without a build path.
  Preserve existing source/manual setup commands and native consent requirements.
- Add offline packaged-install qualification, synthetic installer failure cases,
  installed CLI/MCP transport test seams, and public installation/signing docs.
- CLI/native host, MCP and service advance to `0.5.0`. Core `0.1.3`, primitives
  `0.1.1`, protocol/effect `0.4.0`, extension `0.3.0`, wires and MagicRun are
  unchanged; Magician requires no source/API/storage migration.
- Public registry, signed/quarantined-download and real native setup/extension
  acceptance remain separate release gates. See [distribution](docs/distribution.md).

## 0.4.0 — 2026-09-07 — Process and HTTP delivery source alpha

Source-only alpha; no registry, binary, extension-store or production release is
announced. [Qualification and limits](docs/qualification/results-delivery-2026-09-07.md).

### Added

- Reference-only `secure_new_process` and `secure_new_http` through the daemon,
  shipped CLI and MCP, with caller-owned fixed destination profiles, separate
  human registration and fresh native per-use consent.
- MagicRun-backed governed batch execution with fixed executable/arguments/cwd,
  approved executable digest revalidation, clean environment, env/stdin delivery,
  bounded output and owned-child cancellation/cleanup.
- HTTP methods and header/query/text/form/flat-JSON placements; public HTTPS
  with vetted/pinned DNS and verified TLS, explicit loopback IP HTTP for local
  development, no ambient proxy/redirect/retry and bounded response discard.
- Receipt-only results withholding stdout/stderr/exit codes and HTTP response
  content/headers/raw status codes, including encoded recipient echoes. Typed
  durable audit, spent-ID protection and status reconciliation after audit failure.
- Local HTTP/TLS, process, broker and actual CLI/MCP conformance coverage, focused
  `make test-delivery`, reference-only examples and a public delivery usage guide.

### Compatibility and security

- Standalone packages and test support advance to `0.4.0`; local agent wire is
  now `3`. Upgrade daemon/CLI/MCP together. Existing browser permission data is
  retained and delivery profiles start absent; old standalone binaries may reject
  the extended registry. Profiles persist, but effect jobs do not survive restart.
- Core `0.1.3`, primitives `0.1.1`, extension `0.3.0` and native wire `1` are
  unchanged. No Magician source/dependency update or MagicRun runtime change is
  required. The standalone effect crate consumes MagicRun's existing public
  `tool-runtime-core 0.1.73` coordinator, with reviewed Git source in Cargo.lock.
- This is not a process sandbox or a promise that recipients cannot copy secrets.
  Existing services/PIDs/PTYs, private HTTPS, response-content access and other
  tools' observations remain unsupported. Native acceptance and performance
  measurement are separate gates, not implied by automated conformance.

### Verification

- Full MagicVault Rust suite: 199 passed; six additional disposable headed/
  headless/CLI browser cases, 17 JS cases and 23 tooling regressions passed.
  One optional public-site case was not rerun. Optimized standalone build and
  all-target compilation passed; no native installed-extension/keychain or
  performance qualification is implied.
- Ten optimized CLI/MCP integration and logging-boundary reruns passed; these
  overlap, rather than increase, the distinct case counts above.
- Unchanged MagicRun runtime: 486 Rust and 23 tooling cases passed, with full
  compilation/build. Magician's own runtime suite was not run.

### Development

- Route Makefile Cargo build/check/test artifacts to SSD1 when available, with
  a checkout-local fallback and explicit target/volume overrides. Add a
  `print-target-dir` helper and isolated routing tests. Existing caches, stores
  and runtime behavior are unchanged.
- Add a version-bound architecture diagram/document and an explicit source/doc
  drift gate, included in `make check`, with synthetic checker regression tests.

### Documentation

- Label Rust edition 2021 and the standalone MCP compiler requirement of 1.88+
  separately; explain the SDK requirement and the recorded qualification toolchain.
- Restructure the README around the product, with a Bash quick start, CDP and
  extension setup choices, an MCP configuration example, and explicit coverage
  for existing browsers, new HTTP requests, processes, terminals and stateful
  services. Separate available interfaces from unimplemented credential-delivery
  destinations. Use the headline “Keep secrete away from Agents” while retaining
  the explicit security boundary.
- Remove planning and implementation-review journals from the current docs tree;
  keep technical architecture, contracts, setup and acceptance documentation.
- Clarify when embedded consumers need a shared-library update. No runtime,
  protocol or shared-library behavior changes accompany these documentation edits.

## 0.3.0 — 2026-09-07 — Browser-delivery source alpha

Source version for browser credential delivery and its qualification baseline.
This entry does not announce a registry publication, GitHub release, installer
or Chrome Web Store listing. It is not a production-qualified release.
See the [version and distribution policy](docs/versioning.md),
[architecture](docs/architecture.md) and [qualification](docs/testing.md).

### Verification

- Workspace/all-target compilation, optimized standalone builds and extension
  source packaging passed on macOS arm64. Earlier extension syntax checks and
  release-mode CLI/MCP logging safeguards also passed.
- Final qualification: **195 distinct cases passed** — 171 default Rust tests,
  12 extension JavaScript tests, five local HTTP/child-process fixture contracts,
  and seven explicitly enabled real-browser/CLI/public-page tests. The full Rust
  suite took 17.96 seconds with the warm cache; this is not a latency benchmark.
- Real Chrome coverage includes headed/headless fills, strict controls/partial
  outcomes, navigation, frames, an actual CLI-to-daemon-to-Chrome flow and a
  synthetic public-page smoke test. Process fixtures do not implement planned
  product effects. See the [current qualification record](docs/qualification/results-2026-09-07.md).
- Installed-extension/native-human/keychain, measured performance and broader
  platform qualification remain outstanding; CDP tests do not certify an installed
  extension workflow.

### Added

- Reference-only `secure_fill`, browser/target discovery, fill status and
  cancellation through the standalone daemon, CLI and official-SDK MCP surface.
- Explicit per-client credential-field and exact-origin browser permissions;
  one-use, document-bound native human approval for each fill.
- `magicvault-effect` with a bounded dedicated CDP adapter and trusted native
  bridge. No proxy, arbitrary JavaScript or generic CDP model tool.
- Credential-focused Chromium extension, explicit site permissions, native host
  executable, and scoped macOS installation/removal commands.
- Partial/uncertain outcomes, spent operation IDs, typed durable completion
  receipts and post-audit-failure status lookup without permitting new effects.
- Synthetic protocol, transport, daemon, CLI/MCP, native-host and extension
  fixtures; explicit opt-in headed/headless browser qualification sources.
- Public setup, usage, security, integration and current-versus-roadmap guidance.
- Direct README extension installation steps, shared synthetic browser pages,
  bounded process/IPC destination fixtures, and browser/CLI/extension/process
  acceptance runbooks with an explicit results template and remaining-gates table.
- Compile-time dependency-log suppression in shipped executables, document-origin
  and changed-control checks, and explicit trusted-embedder logging requirements.

### Compatibility

- Standalone CLI/daemon, MCP, service, effect and protocol crates and the Chromium
  extension use `0.3.0`; the prior standalone application version was `0.2.2`.
  The test-only helper follows `0.3.0` and remains unpublished.
- Standalone local protocol advances to version 2. Upgrade 0.3.x daemon/clients
  and extension/native-host assets together; mismatches fail closed. Browser
  permissions are absent by default for existing credentials. A standalone
  downgrade may reject the new registry. Follow the
  [upgrade and recovery instructions](docs/setup.md#upgrade-and-recovery).
- Shared core `0.1.3` and primitives `0.1.1` source, vault format, key identity
  and embedded-consumer dependencies are unchanged by this update.
- Browser delivery does not filter other tools' later DOM, screenshots, cookies
  or session artifacts. HTTP, new-process and existing-service effects remain
  unimplemented. Publication/distribution qualification is separate.

## Shared-library compatibility

Core `0.1.3` adds opt-in durable typed audit append behavior. Existing consumer
append-only APIs retain their previous behavior. Primitives `0.1.1` preserve
private staging-file permissions and relative-path durability. Browser delivery
does not require embedded consumers to adopt the standalone surfaces.
