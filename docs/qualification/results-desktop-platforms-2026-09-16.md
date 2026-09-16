# Desktop prompt and platform source checks — 2026-09-16

Candidate: local `0.9.0` work based on `7fd0731`, including the shared prompt
window and new Linux/Windows standalone backends. This record is not a public
release, remote CI result, or complete desktop acceptance certificate.

## Executed checks

| Environment | Check | Result |
| --- | --- | --- |
| macOS arm64, Rust 1.92 | CLI, MCP and real desktop renderer compile | Passed |
| macOS arm64 | Existing primitives/prompt/service library checks, with loopback-dependent delivery tests rerun outside the filesystem sandbox | Passed; initial sandbox listener failures were environmental |
| macOS arm64 | Two new helper-process tests: typed replies, unsuccessful child refusal, cancellation/reaping and no launch after pre-cancellation | 2 passed |
| Isolated Debian Bookworm arm64, Rust 1.92 | CLI/MCP/service/desktop `cargo check --locked`, then actual CLI/MCP/prompt builds | Passed |
| Same Linux container | `cargo test --locked -p magicvault-primitives -p magicvault-prompt -p magicvault-service --lib` | 98 passed: 20 primitives, 2 prompt, 76 service |
| Same Linux container, Xvfb/Openbox/Mesa llvmpipe | Real prompt executable with synthetic metadata/input over its production pipe protocol | Input window rendered; value masked; Continue returned the exact synthetic value only through private stdout, with no value in stderr |
| Same virtual desktop | Final one-time approval layout | Visually inspected; summary, details disclosure and Use once/Cancel controls visible |
| macOS cross-check for `x86_64-pc-windows-gnu` | CLI/MCP/prompt desktop source and test targets; primitives/service test targets | Passed type/compile checks; no Windows native execution |
| Node 22+ on macOS | Distribution, SDK and six-platform package selection/integrity tests | 39 passed |
| Node 22+ on macOS, npm launch preparation | The above checks plus real offline tarball packing and release integrity/order/failure tests | 44 passed; registry publication is simulated and no package was uploaded |

The Windows test targets include protected-file publication and rejection of
world access, same-user pipe peers, first-instance exclusivity, non-consuming
liveness/disconnect, junction activation/retirement, and literal scheduled-task
path quoting. They still need execution on Windows. Merely compiling those tests
is not evidence that they passed there.

The Linux broker cases include saved and one-time fills, no enrollment during
one-time use, empty-vault use, cancellation/expiry, exact final field mappings,
status-only receipts, uncertainty and no replay. These use source-owned test
adapters and synthetic humans; they do not constitute a real Codex/browser demo.

## Findings and retained limitations

- The minimal Linux image needed `libxkbcommon-x11-0` at runtime. Missing it
  refused the renderer. The dependency is now documented and included in the
  platform workflow's Linux setup.
- The screenshot utility retained old files under numbered names. The first
  blank capture preceded the dependency fix; the later rendered captures were
  inspected directly. Interactive test-driver waits expired while inspection
  was in progress; those attempts are not successful approval tests.
- The disposable container subsequently stopped responding to exec/copy/stop.
  Remaining final-approval/parent-EOF UI automation and real Secret Service/daemon
  smoke checks were not completed. The completed library and masked-input results
  above are separate from that infrastructure failure.
- No native Windows desktop, Credential Manager, Task Scheduler or installed
  browser-registration acceptance ran. No Windows ARM64, Intel macOS or Linux x64
  binary was executed. Six-platform packaging tests use synthetic executable
  headers, not six qualified releases.
- Linux systemd login-session startup, real session-keyring access, Wayland and
  installed-browser acceptance remain open. Xvfb exercises X11 rendering only.
- Windows process delivery remains `unsupported_target`; MagicRun needs its own
  qualified Windows backend. Windows file/junction crash durability also needs
  target-filesystem acceptance; no Unix directory-fsync equivalence is claimed.

The new `desktop-platforms.yml` workflow defines native macOS/Linux/Windows build
and library-test runners. It was not dispatched or reported green in this pass.
The original 0.8.3 saved-fill demo remains historical. A subsequent
[real macOS 0.9.0 demo](../demo.md), built from `d00398c`, records optional
login/card enrollment followed by a just-in-time login with native human input
and approval. Its successful website result and value-free transcript audit are
separate evidence from the source/container checks above. It does not exercise
saved-credential delivery with the new renderer or complete platform acceptance.

Raw fixture screenshots and local harness material belong in ignored `output/`.
No personal vault, account, live service, Magician runtime or existing browser
profile was used for these checks.
