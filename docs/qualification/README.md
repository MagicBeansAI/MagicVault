# Real-world qualification

Start with the [release acceptance checklist](release.md) for current readiness,
the [testing guide](../testing.md) for automated lanes and coverage, or a runbook
below for a specific surface.

Automated cases are integration/conformance tests; cases requiring a person to
interact with native UI are acceptance tests. Neither proves how an agent model
will choose tools. Fixture-only running-service tests do not implement a product
refresh/rotation adapter.

## What is covered

| Surface | Automated lane | Acceptance / reproduction |
| --- | --- | --- |
| Headed/headless Chromium through CDP | `make test-browser-native` | [Browser runbook](browser.md) |
| CLI and daemon | `make test-cli-native` | [CLI runbook](cli.md) |
| MCP, native host and two independent extension profiles | `make test-extension-native` | [Transport commands](extension-transport.md), [native extension acceptance](extension.md) |
| New process and HTTP delivery | `make test-delivery`, `make test-delivery-latency` | [Delivery runbook](process.md) |
| Prebuilt packages and managed installation | `make test-distribution`, `make test-package-install`, `make test-package-browser` | [Native installation lifecycle](distribution.md) |
| Bounded service capacity and shutdown | `make test-service-reliability` | [Measurement scope](../testing.md#performance-recovery-and-consumer-gates) |
| Public demonstration pages | Opt-in `make test-public-web` | [Non-gating smoke test](browser.md#optional-public-page-compatibility-smoke) |

Real-browser automation uses disposable profiles and synthetic consent/key
providers. Only separately recorded human acceptance qualifies native dialogs,
keychain interaction and OS-user installation.

## Evidence records

The latest independently downloaded candidate is **0.8.3 on `81fe8c6`**.
Its automated CI passes do not close the intermittent browser/extension failures,
remaining native recovery cases, publisher verification or public distribution.
See the [release gates](release.md#gates-and-evidence).

These records retain original revisions, commands, failures and limitations.
Older passes do not qualify changed packages; later passing trials do not erase
earlier failures. They are technical evidence, not a task queue or roadmap.

### Current candidate and browser reliability

- [0.8.3 downloaded candidate, CI and installed tests](results-broad-candidate-2026-09-08.md)
- [Browser evaluation, discovery and settlement failures; later passes](results-browser-command-2026-09-08.md)
- [Native macOS launch correction and 200 concurrent trials](results-native-spawn-2026-09-08.md)

### Version-specific native acceptance

- [Browser permissions, pause/reconnect and in-flight cancellation](results-permission-recovery-2026-09-08.md)
- [Cancellation receipts and installed recovery](results-cancellation-2026-09-08.md)
- [Process/HTTP native consent modes, revocation and persistence](results-consent-2026-09-08.md)
- [Large-profile discovery and installed keychain/browser acceptance](results-discovery-2026-09-08.md)

<details>
<summary>Earlier conformance and investigations</summary>

- [Owned-child pre-exec stage observations](results-launch-stage-2026-09-08.md)
- [Owned-child OS exit-reason observations](results-os-exit-reason-2026-09-08.md)
- [Focused process failure and separate downloaded-candidate tests](results-focused-candidate-2026-09-08.md)
- [Owned-child signal and cleanup observations](results-process-signal-2026-09-08.md)
- [Exact installed process path and browser resource observations](results-exact-path-2026-09-08.md)
- [External-volume native-host startup limitation](results-startup-policy-2026-09-08.md)
- [Completion/admission race and correction](results-completion-order-2026-09-08.md)
- [Initial process/native-host reliability investigation](results-reliability-2026-09-08.md)
- [0.8.1 distribution CI: retained failure and later pass](results-distribution-ci-2026-09-08.md)
- [0.7.0 broader conformance and distribution CI](results-release-2026-09-08.md)
- [Chromium document-ID correction and native transport](results-native-transport-2026-09-07.md)
- [Automatic independent browser connections](results-extension-connections-2026-09-07.md)
- [Site grants, blocklist and access indicators](results-extension-access-2026-09-07.md)
- [Initial prebuilt package and app-only lifecycle](results-distribution-2026-09-07.md)
- [Initial process/HTTP delivery](results-delivery-2026-09-07.md)
- [Initial browser/CLI and synthetic fixture qualification](results-2026-09-07.md)

</details>

## Safety rules

- Use synthetic values, a new private vault root, and fresh disposable browser
  profiles. Never point tests at a personal profile or an existing runtime root.
- Normal installer manifests and keychain identities are OS-user state. Use a disposable
  OS account for installed-extension/keychain qualification, or obtain explicit
  permission before touching those exact definitions in a normal account.
  The automated native-transport fixture instead installs its test-only manifest
  inside a wholly new user-data root; it never invokes the normal host installer
  or keychain. A named profile inside a personal user-data root is not equivalent.
- Never bypass native consent to qualify native consent. Automated CLI tests use
  an in-process synthetic human/key provider and explicitly do not qualify that UI.
- Never submit public-site forms, authenticate to a real account, purchase, upload,
  or run destructive actions. The optional public test uses a fixed synthetic
  value and blocks submit locally. Public site scripts still see that fake value.
- Keep raw logs, profiles, capabilities, dumps and screenshots out of Git. Use
  ignored `output/` for local artifacts; review/redact before sharing anything.
- Stop only a browser/server/child created by the test. No `kill-all`, broad PID
  discovery, profile deletion, store adoption or key regeneration is part of a run.
- A partial/uncertain outcome is not a retry instruction. Native/browser failure
  must fail the gate rather than switch to raw credential retrieval or typing.

## Evidence format

Copy [the result template](result-template.md) for a new qualification record.
Identify source revision (or uncommitted working tree), lockfile/toolchain,
OS/browser versions, commands, cases, durations and observed failures. Record
`PASS`, `FAIL`, `BLOCKED`, `NOT RUN` or `NOT IMPLEMENTED`; never count a skipped
or fixture-only case as a passing product feature.

Synthetic site/child fixtures are test-support code and are not linked into a
production executable. They must never become a production fake-consent or
plaintext-reading backdoor. Shared fixture helpers keep the browser owner and
cleanup explicit, including cancellation/panic paths.
