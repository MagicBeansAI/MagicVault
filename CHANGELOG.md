# Changelog

## 0.3.0 — 2026-09-07 — Browser-delivery source alpha

Source version for the Phase 3 implementation and qualification checkpoint.
This entry does not announce a registry publication, GitHub release, installer
or Chrome Web Store listing. It is not a production-qualified release.
See the [version and distribution policy](docs/versioning.md),
[review ledger](docs/phase3-review.md) and [qualification](docs/testing.md).

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
  synthetic public-page smoke test. Process fixtures do not implement Phase 4
  product effects. See the [current qualification record](docs/qualification/results-2026-09-07.md).
- Installed-extension/native-human/keychain, measured performance and broader
  platform qualification remain outstanding. The [earlier evidence](docs/phase3-verification-2026-09-07.md)
  remains a historical record, not an installed-extension certification.

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
  and embedded-consumer dependencies are unchanged by this phase.
- Browser delivery does not filter other tools' later DOM, screenshots, cookies
  or session artifacts. HTTP, new-process and existing-service effects remain
  unimplemented. Publication/distribution qualification is separate.

## Earlier library and foundation checkpoints

The [foundation ledger](docs/phase2-foundation.md),
[targeted synthetic results](docs/targeted-tests-2026-09-06.md), and
[durable-audit review](docs/deep-review-2026-09-07.md) retain versioned historical
evidence. Those results do not qualify the new browser implementation.
