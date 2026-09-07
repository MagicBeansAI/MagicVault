# Native cancellation receipt qualification — 2026-09-08

Source: uncommitted `0.8.1` correction on `2c7d0f5`, following the uncommitted
`0.8.0` [consent implementation and native acceptance](results-consent-2026-09-08.md).
CLI/MCP/service are `0.8.1`; protocol/effect remain `0.6.0`, agent wire `4`,
extension `0.6.1`, native handshake `2` and bridge/config schema `1`.
Shared core/primitives, MagicRun and Magician are unchanged.

## Finding and correction

During genuine installed `0.8.0` HTTP acceptance, clearing consent while the
native prompt subprocess was active prevented delivery and removed the prompt.
Recipient counters stayed at four, `may_have_run` was false and no grant appeared.
However, the terminal receipt reported `denied`, indistinguishable from a human
Deny choice. This is a failed cancellation-receipt case, not a credential leak.

The native prompt provider can return `Denied` during cancellation teardown.
Browser fills already normalize that result against their cancellation token;
process/HTTP delivery did not. The correction applies the same ordering before
custody resolution: preserve explicit `Expired`, otherwise map an active broker
cancellation to `Cancelled`, and retain genuine provider errors when not
cancelled. Successful authorization, effect dispatch, post-dispatch uncertainty
and durable-write failure handling are unchanged. The native provider itself
and shared custody APIs were not modified by this correction.

Three added regression tests cover both process and HTTP, including explicit
cancel and consent reset, genuine denial without cancellation, and explicit
expiry racing teardown. Negative recipient observations check that no child
marker or HTTP connection appears and no consent grant is created. Before the
fix, the focused run reproduced the mismatch: one test failed with `Denied`
instead of `Cancelled`; the denial and expiry tests passed.

## Requalification status

The corrected full workspace, packaged clients, two dedicated native
pending-consent cancellation paths, subsequent human denial and process-grant
restart have been requalified. The wider acceptance matrix remains incomplete.
No `0.8.0` pass is silently treated as a `0.8.1` result.
Only synthetic local recipients are in scope. Native approval/password decisions
remain human-controlled; prompt-process diagnostics expose only the count of
`osascript` children of the exact dedicated service, never dialog text,
arguments, launchd environment or capabilities. Process-count observations do
not independently qualify visual dialog rendering or a human late click.

| Check | Result | Scope |
| --- | --- | --- |
| Full locked, offline Rust workspace | **PASS**, 251 passed, 9 opt-in tests ignored | Includes the three added cancellation/denial/expiry tests across both delivery kinds |
| JavaScript suites | **PASS**, 84 tests | One initial fixture was blocked by sandbox `listen EPERM`; the unchanged suite passed with isolated loopback permission |
| Release build | **PASS**, 23.05 s | CLI/MCP release binaries, SSD1 target directory |
| All-target workspace check | **PASS**, 3.08 s | Locked, offline dependencies |
| Architecture/build-path guards | **PASS**, 24 tests | Architecture baseline matches 68 source inputs; store-durability guard reports no new violations |
| Offline packaged qualification | **PASS**, 13 reused client/host tests | Local npm installation, installed assets, app-only upgrade, stable binaries after npm removal and recoverable app-only uninstall; browser tests not requested for this run |

The npm tarballs are local, unsigned qualification artifacts, not published
registry packages. SHA-256 identities:

| Input | SHA-256 |
| --- | --- |
| Launcher `0.8.1` | `f60786c524ec9873504d85f03b7820efd190085156767ddec2576e6820dfdb53` |
| macOS arm64 native `0.8.1` | `8b781ed22f02ed3bd808d6c0e470b5c0ec17d23fb49bc0e98c48fc758258ff1f` |
| `Cargo.lock` | `1d4354326e498932da452c5c41a4cfb61b2956f750bd7caf2b4573a6a5048a32` |

No product policy or test deadline was weakened to obtain these passes.

## Dedicated installation acceptance

Pre-upgrade doctor confirmed the dedicated `0.8.0` daemon ready, the retained
client, verified application/native-host definitions and one connected extension
profile. No native consent subprocess was pending; both HTTP fixture counters
remained at four.

The matching `0.8.1` tarballs installed offline with npm scripts disabled.
Managed upgrade then returned `transport_uncertain` while waiting for readiness.
Read-only reconciliation confirmed that `0.8.1` application files were active
and integrity-verified, and native-host definitions still matched the selected
profile, but the daemon remained `transport_unavailable`. Its owned process was
present with no `osascript` consent child.

The human then confirmed entering the password locally in the macOS Keychain
prompt and selecting Always Allow. Doctor subsequently reported a ready daemon
under a new epoch, with the same paired client and both registered delivery
profiles preserved. The credential-use grant list remained empty: Keychain
approval is not MagicVault remembered-use consent. The extension initially had
zero connected profiles, then reconnected automatically; a later doctor reported
one connected profile with verified native-host definitions and app integrity.
No upgrade retry, key replacement, vault initialization, browser restart or
automated approval occurred. This is successful readiness reconciliation after a
human Keychain decision, not a claim that the original upgrade returned success
or that the earlier extension-startup test flake is fixed.

| Native case on installed `0.8.1` | Result | Observation |
| --- | --- | --- |
| HTTP pending-consent reset | **PASS** | Native prompt subprocess count changed 0 → 1 → 0; `clear-consents` acknowledged reset; the same operation settled as `cancelled`, error `cancelled`, `may_have_run: false`; both HTTP counters stayed at four and no grant appeared. The human also confirmed the dialog disappeared. |
| Process pending-consent explicit cancellation | **PASS** | A fresh request reached native consent, with one prompt subprocess; `cancel-delivery` initially returned a pending receipt, then status for that same ID settled as `cancelled`, error `cancelled`, `may_have_run: false`; prompt subprocess count returned to zero, execution markers stayed at three and no grant appeared. |
| Fresh human denial after cancellation cleanup | **PASS** | Human confirmed Deny on a separate fresh process request; the same operation settled as `denied`, error `denied`, `may_have_run: false`; execution markers stayed at three, no grant appeared and no native consent subprocess remained. |

Neither pending request was approved. Receipt reconciliation queried existing
operation IDs and never replayed an effect. The human-confirmed HTTP dismissal
does not qualify a late approval race or all native dialog rendering; synthetic
late-provider tests remain separate evidence. These cases cancel before dispatch
and do not qualify in-flight process termination or HTTP uncertainty.

## Additional native recovery

Process remembered-grant persistence across daemon restart: **PASS** on installed
`0.8.1`.

| Step | Observation |
| --- | --- |
| Genuine Always allow | Human confirmed the selection on a separate fresh process use; it completed, the marker count increased from three to four, and one exact-process-profile grant appeared. No native prompt remained. |
| Controlled restart | Checked the exact private LaunchAgent definition, including executable, root, label and all arguments. Stop acknowledged unloading and read-only status returned `transport_unavailable`; start loaded the same definition. Doctor reported a ready daemon under a new epoch, the same client and one reconnected extension profile. No browser restart, key replacement or additional native approval was performed. |
| Persistence and no replay | The same grant ID and exact profile scope survived. Marker count remained four through restart. The old completed operation returned `not_found` when queried; it was not resubmitted. |
| Fresh remembered use | One deliberately new matching operation completed without another human approval; markers increased from four to five, and no native prompt was pending. CLI output remained closed receipt metadata, withholding the recipient's intentional stdout/stderr echo. |
| Cleanup | Revocation of only this test's exact grant was acknowledged; listing returned no remembered grants. The registered profiles, vault, pairing and running dedicated daemon were retained. |

This qualifies settled-work restart and remembered-process reuse on this host,
not an in-flight crash, restored operating-system session, or broad service-outage
matrix. HTTP remembered-grant restart was separately qualified on `0.8.0`; its
result has not been relabeled as a `0.8.1` execution.

Public HTTPS/provider coverage, broad permission/recovery matrices, maximum
dialog readability, sustained performance, signing/notarization, registry
publication and public release gates remain open. The earlier intermittent
native-extension startup timeout is not addressed by this receipt correction.
