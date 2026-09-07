# Real-world qualification

These are **integration/conformance tests** when automated and **acceptance
runbooks** when a person must interact with native UI. They are not model-quality
evals. A future eval could measure whether an agent selects `secure_fill`, but
that would not replace destination, custody, transport or output-boundary tests.

## What is covered

| Surface | Runnable evidence | Remaining gate |
| --- | --- | --- |
| Real headed/headless Chrome through CDP | `make test-browser-native` | More brands/platforms and accessibility/control combinations |
| Actual CLI → daemon → real Chrome | `make test-cli-native` | Genuine native dialogs/keychain in [CLI runbook](cli.md) |
| MCP and native-host executables | Existing real subprocess/IPC tests | Installed-extension/native UX in [extension runbook](extension.md) |
| Public demo pages | Opt-in `make test-public-web` | Non-gating compatibility smoke, not login/provider qualification |
| New process / HTTP requests | `make test-delivery`: real local HTTP/TLS and MagicRun children, shipped CLI/MCP | Native profile approval/keychain and measured performance; see [delivery runbook](process.md) |
| Running-service destinations | `make test-qualification-fixtures` exercises a cooperative Node service | Product refresh/rotation integration remains **unimplemented** |
| Prebuilt CLI/MCP installation | `make test-distribution` and offline `make test-package-install` | Developer ID/quarantined downloads, registry publication and full native lifecycle in [distribution runbook](distribution.md) |

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
- Native manifests and keychain identities are OS-user state. Use a disposable
  OS account for installed-extension/keychain qualification, or obtain explicit
  permission before touching those exact definitions in a normal account.
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
