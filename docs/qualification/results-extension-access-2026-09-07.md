# Extension site-access qualification — 2026-09-07

Scope: extension `0.4.0` source changes on base commit `bdf6c0d`, with reviewed
production-source fingerprints in [the architecture baseline](../architecture-baseline.json).
This is local development evidence, not a store release or production certification.
No Rust crate, native wire, core, MagicRun or Magician source was changed.

## Automated evidence

Environment: macOS 26.6, arm64, Node 22.19.0. No Rust build was needed.

| Command / lane | Result | Scope |
| --- | --- | --- |
| `node --test extension/tests/*.test.cjs scripts/tests/distribution.test.mjs` (`make test-extension`) | 41 passed | Eight fixed-fill cases, nine setup-page cases, 16 worker cases, eight distribution cases |
| `python3 scripts/tests/test_architecture.py` | 12 passed | Synthetic architecture-drift regression fixtures |
| `python3 scripts/tests/test_build_paths.py` | 12 passed | Fake-Cargo build routing; no Rust execution |
| `python3 scripts/check_architecture.py` | PASS | Reviewed baseline matches 63 production inputs |
| `node --check extension/worker.js` and `node --check extension/options.js` | PASS | JavaScript syntax |
| `actionlint -shellcheck= .github/workflows/qualification.yml .github/workflows/distribution.yml` | PASS | Local workflow syntax/context validation, actionlint 1.7.12; no hosted dispatch |
| `make package-extension` | PASS | Assembled unpacked assets, including canonical shared `fill.js` |

The JavaScript fixtures execute actual worker, setup and fill sources with
synthetic Chrome APIs/DOM. The distribution fixture boots the actual assembled
extension directory and deliberately removes `fill.js` to prove imports are not
mocked away. They do not simulate a successful native credential enrollment.

Access coverage includes broad permission approval/denial, selected-site reset,
separate local HTTP, retained legacy grants, exact top/frame blocks, host/port
normalization, worker recreation, concurrent settings writers, malformed or
unavailable storage, storage access restriction failure, pre-dispatch revocation
with and without change notifications, late-change semantics and pending prompts.
Discovery reads storage once per enumeration; no latency benchmark is claimed.

## Disposable Chrome smoke check

A separate headless Chrome profile loaded `dist/extension` through the browser's
development extension-loading API, with Playwright CLI 0.1.18. No normal browser
profile, native-host registration, vault, keychain or daemon connection was used.

- Actual worker registration, shared script import and settings rendering passed.
- Adding a synthetic `https://example.com` block and re-enabling the selected entry
  worked through the page and actual extension local storage.
- Clicking all-HTTPS reached a pending browser permission request; no host grant
  was issued automatically. The browser was closed to cancel the pending prompt.
- Updated assets loaded again in the disposable browser; its page console reported
  zero errors/warnings. The development-loaded extension needed explicit loading
  again after browser restart, so this is not normal installed-extension lifecycle
  qualification. Persistent worker settings are covered by the synthetic suite.

The native browser permission prompt was **not approved** by automation. Successful
real broad-site grants, browser-enforced revocation, normal installed-extension
restart/upgrade behavior and native-host/credential-fill acceptance remain the
separate [X-10–X-15 and existing native acceptance cases](extension.md).
The disposable browser was closed. No publication, signing, commit or push is
part of this qualification.

## Access-indicator follow-up

The same focused JavaScript/package command passed **47 cases** after making
the current browser grant prominent and independent of blocklist availability.
Six added setup cases cover reopening with existing permission, no-access/denial,
slow or failed blocklist reads, failed Chrome permission reads and recovery,
permission changes/page return, and stale asynchronous reads. The existing
approval/reset cases now assert the banner state and enable-button label/state.
Syntax and reviewed architecture validation also passed. This follow-up was
verified with synthetic Chrome APIs; the earlier disposable-browser smoke check
does not qualify the new indicator or native permission approval visually.

The subsequent ID-chip change passed **48 focused JavaScript/package cases**,
adding coverage that the highlighted, keyboard-focusable code displays the actual
runtime extension ID without initiating setup or connection. Packaging and the
reviewed architecture check also passed. No connection automation or native-host
registration change accompanies that visual update.
