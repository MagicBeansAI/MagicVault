# Real-world qualification

These are **integration/conformance tests** when automated and **acceptance
runbooks** when a person must interact with native UI. They are not model-quality
evals. A future eval could measure whether an agent selects `secure_fill`, but
that would not replace destination, custody, transport or output-boundary tests.

## What is covered

Use the [public release gate matrix](release.md) to separate automated conformance,
human desktop acceptance and publisher/registry authorization. The
[0.8.1 distribution CI record](results-distribution-ci-2026-09-08.md) includes a
successful unsigned package/install workflow on `43b6a1f`, while retaining the
first run's unresolved intermittent process-delivery uncertainty. The
[installed permission/recovery record](results-permission-recovery-2026-09-08.md)
adds genuine remembered-use permission refusal/restoration, pause/resume and
process/HTTP in-flight cancellation on committed `34fee4a`. The
[0.8.1 cancellation record](results-cancellation-2026-09-08.md) records the current
receipt correction, automated/package passes, installed native pending-consent
cancellation and process-grant restart/reuse passes. The
[0.8.0 consent results](results-consent-2026-09-08.md) record local/native
consent conformance, unresolved startup failures and the original reset-receipt
failure. The
[broader release results](results-release-2026-09-08.md) apply only to the prior
committed `0.7.0` revision, including CI and repeated fresh browser starts.

| Surface | Runnable evidence | Remaining gate |
| --- | --- | --- |
| Real headed/headless Chrome through CDP | `make test-browser-native` | More brands/platforms and accessibility/control combinations |
| Actual CLI → daemon → real Chrome | `make test-cli-native` | Genuine native dialogs/keychain in [CLI runbook](cli.md) |
| MCP and native-host executables | Real subprocess/IPC tests; `make test-extension-native` adds actual Chrome dispatch and two independent profiles; [basic installed native acceptance](results-discovery-2026-09-08.md) on one host | Remaining permission/recovery cases in [extension runbook](extension.md); wider hosts and live agent-model sessions not yet qualified |
| Public demo pages | Opt-in `make test-public-web` | Non-gating compatibility smoke, not login/provider qualification |
| New process / HTTP requests | `make test-delivery`: real local HTTP/TLS and MagicRun children, shipped CLI/MCP; `make test-delivery-latency` adds local repeated-call observations; native consent and in-flight results linked above | Remaining native recovery, wider hosts, Internet latency and resource/soak performance; see [delivery runbook](process.md) |
| Running-service destinations | `make test-qualification-fixtures` exercises a cooperative Node service | Product refresh/rotation integration remains **unimplemented** |
| Prebuilt CLI/MCP installation | `make test-distribution`, offline `make test-package-install`; opt-in `make test-package-browser` adds installed extension assets | Developer ID/quarantined downloads, registry publication and full native lifecycle in [distribution runbook](distribution.md) |

The [0.7.0 discovery record](results-discovery-2026-09-08.md) covers large-profile
regressions, rebuilt packages and installed-candidate acceptance. The
[0.6.1 transport record](results-native-transport-2026-09-07.md) identifies the
earlier native dispatch fix, executed lanes, small-sample latency and remaining
release gates. [Repeatable native transport commands](extension-transport.md)
keep automated synthetic qualification separate from human acceptance.

The [browser runbook](browser.md) describes the local pages, exact commands and
outcome checks. [The dated results](results-2026-09-07.md) distinguish actual
execution from manual and future gates. The [0.4.0 delivery record](results-delivery-2026-09-07.md)
covers delivery. The [0.5.0 distribution record](results-distribution-2026-09-07.md)
covers packaging/onboarding. MagicRun is the standalone new-process dependency;
the shared custody contract and Magician remain unchanged.
See the [architecture](../architecture.md).

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
