# Owned-child OS exit-reason investigation — 2026-09-08

The [prior focused CI failure](results-focused-candidate-2026-09-08.md#first-ci-failure-retained)
showed an owned child already `SIGKILL`-terminated before cleanup. This follow-up
adds the next missing observation, not a speculative runtime fix or a replay.

## Scope and revisions

The initial MagicVault follow-up builds on `a7794f5` and selects MagicRun
`1ab46d90f7eccddc9817bcce96d027383fa7828e`. Crate/package versions remain
unchanged. Normal builds omit the observer; standard release builds reject its
compiler cfg. Installed clients remain uninstrumented, while the synthetic
broker/test driver enables both diagnostic cfgs.

Only an active thread-local capture, a matching owned child/`SIGCHLD`, and a
signal-exit wait event permit a query. `waitid(WNOWAIT)` retains the child's
identity; at most one `proc_pidinfo(PROC_PIDEXITREASONBASICINFO)` call occurs per
capture, before cleanup/reap. No normal-exit query, retry, process enumeration,
cross-process history or raw crash/log collection is added.

The query accepts only the exact 24-byte packed basic record. Namespace/code are
mapped immediately into closed signal, codesigning, exec or other OS categories.
Raw codes, flags, payload length and process identity are not retained; the
payload is never requested. Missing reason/process, access denial, unsupported
flavor and malformed results remain distinct observations. Non-macOS platforms
record unsupported. Unknown codes cannot escape as arbitrary numbers or strings.
See [MagicRun's ABI sources and boundary](https://github.com/MagicBeansAI/MagicRun/blob/1ab46d90f7eccddc9817bcce96d027383fa7828e/docs/architecture.md#synthetic-process-diagnostics).

Wait/cleanup/timeout decisions, credential custody, public API, protocol and
storage remain unchanged. Literal governed source bytes change because their
test assertions change; future consumer source attestations still require
review. Magician's dependency/source and the live acceptance installation are
untouched. No signing, publication or OS permission bypass is authorized here.

## Local validation

macOS `26.6` arm64; Rust `1.92.0`; build outputs on SSD1.

- The local SDK's `proc_exitreasonbasicinfo` size and all four field offsets
  passed C11 compile-time assertions (24 bytes; offsets 0, 4, 12 and 20).
- Five diagnostic units passed: packed decoding, unavailable/malformed results,
  owned-event/query bounds, thread isolation and global capture capacity.
- Real normal/nonzero exits produced no OS query; self-SIGTERM/SIGKILL produced
  the matching OS signal category before cleanup and matching final reap.
- The explicit deadline-cleanup diagnostic passed separately.
- Normal MagicRun all-target check, 17 batch regressions and eight PTY
  regressions passed. Its 49-input architecture baseline and 11 gate tests passed.
- MagicVault all-target check passed with only the reviewed MagicRun lock change;
  unrelated Windows dependency-edge changes from resolution were restored.
- All five real adapter diagnostic cases passed, including matching OS reason
  categories for self-SIGTERM/SIGKILL. Five normal process and eight HTTP
  regression tests passed. No observer is present in those normal test builds.
- 18 packaging/orchestration tests, 12 architecture tests and the reviewed
  74-input architecture baseline passed.

Retained anomaly: the first local real-signal classification run returned
`TimedOut` where `RuntimeFailure` was expected. The following deadline test
failed on the poisoned test mutex, not an independent runtime finding. The
initial assertion did not retain a snapshot or distinguish which signal case
failed. After adding closed expected-signal/snapshot failure context, the exact
four-case test passed without changing its two-second deadline; the deadline
case passed in a separate invocation. This does not establish or fix the initial
timeout's cause. An intermediate incorrectly qualified exact test filter ran
zero cases and is not counted as evidence.

## First exit-reason CI run

[Run 34191450549, attempt 1](https://github.com/MagicBeansAI/MagicVault/actions/runs/34191450549)
**FAILED** on MagicVault `855b793a773bf46eb935b6e0df71a7a4ab890c79` and
MagicRun `1ab46d90`. Job `101950191034`; macOS `15.7.9` arm64, image
`20260829.0321.1`. Normal client build/package assembly, orchestration/architecture,
the five real adapter classifications and explicit instrumented-release refusal
passed before fresh exact-path process/HTTP trials began.

Trials 1–28 passed both cases. Trial 29 failed the process case while its HTTP
companion passed; the loop stopped after about 23 seconds. No trial 30, retry,
success summary, post-success uninstall or upload followed.

The owned child was again `CLD_KILLED` / `SIGKILL` on the first wait, before
cleanup, with the same signal at final reap. The OS query **succeeded** and
returned `Observed(OtherNamespace)`. Explicit termination cleanup was absent;
the later group kill returned `NoSuchProcess`. The adapter remained dispatched
`RuntimeFailure`, `uncertain` / `unavailable`, `may_have_run: true`; all recipient
markers were absent. No raw namespace/code or payload was retained.

This exposed a diagnostic coverage gap: the initial decoder named likely
process/security namespaces but collapsed other defined Apple namespaces into
the catch-all. The query worked; this is not an unsupported/denied-query result.
No specific namespace can be inferred retrospectively from this record.

### Named-namespace follow-up

The classifier now names every namespace in the reviewed Apple header, including
`INVALID`; only undefined values remain `OtherNamespace`. A complete mapping
regression checks all 47 defined namespace values, the alias and unknown-value
behavior. Six diagnostic units and MagicRun's architecture gate passed. Query
scope, record size, discarded payload/code policy and all runtime behavior remain
unchanged. This justifies one further bounded run with new evidence, not a rerun
of the failed revision or an uncertain operation.

The classifier follow-up selects MagicRun
`e2099b3f69c3f20e75229c6e7b614a63d83db791`. MagicVault's all-target check and
five real adapter diagnostic cases passed again with that exact dependency.
Its CI result is pending in this record. A reason
category narrows an investigation; it does not by itself prove the underlying
bug, its sender or safe retry behavior. All earlier failures remain open.
