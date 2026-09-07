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
| New-process and running-service destinations | `make test-qualification-fixtures` exercises real Node children | MagicVault process-effect integration is **not implemented**; see [process runbook](process.md) |

The [browser runbook](browser.md) describes the local pages, exact commands and
outcome checks. [The dated results](results-2026-09-07.md) distinguish actual
execution from manual and future gates. Phase 3 and this qualification work
change **MagicVault only**; MagicRun and embedded Magician code remain unchanged.

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
