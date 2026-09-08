# Unsigned 0.8.1 distribution qualification — 2026-09-08

Scope: the manually dispatched macOS Apple Silicon distribution workflow,
local-scope npm tarballs and installed CLI/MCP/native-host conformance. These
observations do not qualify signing, native human consent, real browser startup,
public npm installation or a public release.

CLI/MCP/service remain `0.8.1`, protocol/effect `0.6.0`, extension `0.6.1`,
core `0.1.3`, primitives `0.1.1` and locked MagicRun `0.1.73`. No runtime,
dependency, workflow, shared custody or Magician change was made in this round.

## Initial CI failure

[Distribution run 4](https://github.com/MagicBeansAI/MagicVault/actions/runs/34173216073)
ran on committed `527cdbf01040cabd6e8944722b23db52ec24433f` and **failed**.
The runner selected Rust `1.92.0`, Node `22.23.2` and the explicit
`@magicvault-local` package scope. Earlier green `0.7.0` workflows were not
counted as current evidence.

| Stage | Result | Observation |
| --- | --- | --- |
| Distribution JavaScript | **PASS** | 10 tests |
| Architecture and build-path guards | **PASS** | 24 tests; architecture `0.8.1` matches 68 source inputs |
| All-target check and full Rust workspace | **PASS** | 251 passed; 9 explicit opt-in tests ignored |
| Release build and package assembly | **PASS** | Matching `0.8.1` launcher and arm64 native package assembled |
| Installed-client qualification | **FAIL** | Four CLI tests passed, then the process delivery case returned `uncertain` where `completed` was required. The HTTP case passed. Remaining native-host/MCP checks and artifact upload did not run. |

The process assertion originally printed only the state mismatch. Its closed
error code, dispatch flag and recipient marker were not captured, so the failed
run does not establish the cause, whether the recipient completed, or whether
this is a runtime defect versus a fixture/environment issue. It must not be
classified as a timeout, credential leak or successful delivery without evidence.

## Diagnostics and local investigation

Commit `43b6a1fdef31043557de182490631bee5bda7c7e` changes only the failing test's
assertion diagnostic: report the closed error code, `may_have_run` flag and
whether its fixed recipient marker exists. No credential, raw recipient output,
environment, capability or arbitrary diagnostic is included. The assertion still
requires `completed`; operation deadlines, output boundaries, test selection and
normal process/HTTP concurrency are unchanged. This is not a runtime fix.

Using the existing `0.8.1` release CLI and locked offline dependencies locally:

- Initial targeted process/HTTP run: **PASS**, 2 tests; one opt-in latency case
  ignored.
- Thirty independent process-only runs: **PASS**, 30/30.
- Fifteen runs of the complete targeted process/HTTP binary: **PASS**, 15/15,
  each with 2 passing tests and the same opt-in latency case ignored.
- Architecture comparison and whitespace check: **PASS**.

Each run uses newly created disposable roots and synthetic credentials. These
are new fixtures and operations, not retries of the failed CI operation. Build
output stayed on SSD1; the installed acceptance daemon, live vault, keychain,
pairing, acceptance browser configuration and native-host definitions were untouched. Repeated passes are
not proof that the original failure has been fixed.

## Diagnostic CI run

[Distribution run 5](https://github.com/MagicBeansAI/MagicVault/actions/runs/34173788824)
was dispatched on `43b6a1fdef31043557de182490631bee5bda7c7e` with the same
`@magicvault-local` scope and **passed** on its first attempt. GitHub reported
terminal `completed / success` for the workflow and all candidate job steps.

| Stage | Result | Observation |
| --- | --- | --- |
| Distribution JavaScript and guards | **PASS** | 10 JavaScript tests and 24 architecture/build-path tests; unchanged 68-input architecture baseline |
| All-target check and full Rust workspace | **PASS** | 251 passed; 9 explicit opt-in tests ignored |
| Release build and package assembly | **PASS** | Matching `0.8.1` launcher and arm64 native package |
| Offline installed-client qualification | **PASS** | 13 reused CLI/process/HTTP/native-host/MCP tests; one opt-in latency case ignored; app-only setup/upgrade, stable binaries after npm removal and recoverable app-only uninstall |
| Artifact upload and workflow completion | **PASS** | Both local-scope tarballs uploaded in the unsigned candidate artifact |

Counts overlap with previous executions and must not be summed as unique test
coverage. The final qualification report states `passed: true`, `version: 0.8.1`,
`live_custody_or_services_touched: false` and `isolated_browser_tests: false`.
This distribution workflow does not run the separate real-browser lanes or the
full extension JavaScript suite. The separate macOS/Ubuntu **Manual qualification**
workflow was not dispatched in this round.

The [unsigned artifact](https://github.com/MagicBeansAI/MagicVault/actions/runs/34173788824/artifacts/10036672518)
has ID `10036672518`, size 6,528,903 bytes and uploaded ZIP SHA-256
`b4519e39ffacc2ca82013855dbcb29caa4778633926e501aab1add4898dbb5d7`.
The upload log and GitHub artifact metadata agree; this is not an independent
download/content verification or publisher signature. The artifact expires on
2026-09-22 UTC. It is an Actions qualification artifact, not an npm registry
publication or GitHub release.

Subsequent changes in this round are qualification documentation only; the
successful run remains tied to the exact reviewed revision above.

## Remaining limits

The intermittent process-delivery uncertainty and the separately recorded
extension-startup timeout remain reliability findings until their causes and
corrections are established. Passing another workflow alone does not close them.
No test was skipped or weakened to turn the failure green. Full native recovery,
resource/soak measurements, genuine agent-client use, signing/notarization,
registry provenance and public distribution remain in the
[release gate matrix](release.md).
