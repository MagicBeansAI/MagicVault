# MagicVault MCP demo

![Full real MCP setup and Codex login](assets/magicvault-mcp-full-demo.gif)

[Watch the full MP4](assets/magicvault-mcp-full-demo.mp4) ·
[Static poster](assets/magicvault-mcp-full-demo-poster.png) ·
[Actual MCP transcript audit](assets/magicvault-mcp-full-demo-audit.json)

This **1-minute 47-second** edit records the full MCP onboarding and login flow
after MagicVault is installed and its local service is running. It uses a real
Codex CLI session, real native dialogs, and a real Chrome window on
[Practice Test Automation](https://practicetestautomation.com/practice-test-login/).

| Time | Actual recorded step |
| --- | --- |
| 0:00 | Add the MCP server with `codex mcp add`; verify `enabled: true`. |
| 0:07 | Pair the local client and approve the native pairing dialog. |
| 0:14 | Start enrollment, enter the username and password in hidden native fields, receive a credential reference. |
| 0:32 | Configure the exact website origin and approve its field permission. |
| 0:40 | Register the dedicated browser, approve the connection, list references. |
| 0:50 | Ask Codex to log in and allow the MCP tools. |
| 1:02 | Codex calls `secure_fill` with references and selectors; the response is `pending`. |
| 1:08 | The human approves one fill; the browser receives the values. |
| 1:19 | Codex receives a `filled` receipt, submits the form, and verifies the successful login. |
| 1:33 | Inspect the actual credential metadata and run the transcript audit. |

## What the agent received

The final segment runs `scripts/audit-mcp-demo.py` against the **actual login
agent's Codex rollout**. It extracts six MagicVault MCP call expressions and six
replies, including the `pending` and `filled` receipts. The shared JSON report
contains those expressions, replies, timestamps, counts, and the source file's
SHA-256 hash. It contains no vault contents.

Both the practice account's public username and password had **zero literal
matches** in the extracted MCP call source, MCP replies, and the complete
recorded login-agent rollout. A positive-control check confirmed the parser
detects the public password when inserted into a copied response. The audit
reads the recording; it never opens the vault.

This supports the specific recorded result: MagicVault returned references,
field names and status receipts instead of credential values. It is a literal
check of one session, not a general proof against all possible observation or
encoding. The login agent saved browser screenshots to disk without loading
them into its model context; the demo editor separately inspected screen
captures to produce this video.

The practice website publishes its sample credentials on the page. The website
receives the filled values, and separate browser tools can observe them.
MagicVault protects its own output boundary; this recording does not claim that
credentials are inaccessible to every browser tool or to the demo editor.

## What was running

- Codex CLI 0.154.0, connected to the real stdio `magicvault-demo` MCP server.
- MagicVault 0.8.3, its local service, macOS Keychain custody and native consent.
- A fresh `walkthrough` client profile and the public practice account entered
  by the human in native hidden-input prompts.
- A dedicated Chrome profile registered through MagicVault's CDP route.
- `cua-driver` for ordinary browser submission, result verification and recording.

The video begins with installed binaries and a running vault service. It includes
MCP activation, pairing, enrollment, origin permission and browser registration.
Installation itself, extension auto-connection, HTTP/process delivery, and a
separate agent integration are outside this recording. See [Setup](setup.md)
for installation and service startup.

## Source and editing

All application screens and tool results are real captures. Waiting time is cut,
windows are resized, and native dialogs are cropped and slowed for readability.
The last two shots show a real Terminal running the transcript audit after the
login. The clean success shot follows dismissal of Chrome's save-password offer.
This is an edited demonstration, not an uninterrupted desktop recording.

Local source captures and the timestamped edit manifest are in
`output/full-mcp-demo/`, excluded from Git. Only selected demo windows, tight
native-dialog crops, and the value-free MCP audit are shared. The first native
video's original H.264 frames were recovered after its recorder exited without
finalizing the MP4; the recovered clip uses the recorder's intended 30 fps.

With these local source captures present, Python 3, Pillow and FFmpeg reproduce
the edit:

```bash
python3 scripts/render-full-mcp-demo.py --capture-root output/full-mcp-demo
```

Re-run the audit against the local recorded Codex session:

```bash
python3 scripts/audit-mcp-demo.py \
  --rollout /path/to/recorded-codex-rollout.jsonl \
  --output output/full-mcp-demo/transcript-audit.json
```

The audit is specific to this public practice-account recording and its
`magicvault-demo` tool names. Do not supply private secrets as canaries. The
renderer uses the macOS supplemental Arial fonts and this take's source times;
update those paths and times for another recording.

## Record the saved + one-time MCP demo

The checked-in footage is the older saved-credential flow. Record a new real
website/Codex take after the new desktop UI passes acceptance; do not relabel
existing footage as a demonstration of one-time input.

1. Briefly show enabling MCP, pairing, then enrolling a sample login and a
   synthetic card record in masked MagicVault windows. Show only their labels,
   field names and references afterward. No payment or real card data is needed.
2. Ask the actual Codex session to log in to a real test website using the saved
   reference. Record the native destination summary, approval, `filled` receipt,
   browser submission and successful website result.
3. Open a second test login with no saved credential. Show its absence from the
   permitted metadata list, then let Codex call `secure_prompt_fill`. The human
   enters values privately and approves **Use once**. Record the receipt and
   successful website result; show that no credential record was added.
4. Inspect the actual MCP calls/replies and the login agent's transcript for
   both flows. Run positive-control checks with synthetic canaries outside the
   recorded agent session. Publish only a value-free audit report.

Keep the short card-storage glimpse separate from the login demonstrations.
Show real native windows and actual tool results. Record only intended windows;
never load screenshots of unmasked values into the login agent's context. State
that the website receives values and other browser tools can observe them. A
literal transcript audit is evidence for that recording, not proof against every
observation or encoding. Keep any separate Magician/agent demo in its own file.

## Recording setup

Use an isolated vault/client profile and a trusted test account. Start the local
service, enable recording, then add the MCP server. For default installed paths:

```bash
codex mcp add magicvault -- "$HOME/.magicvault-app/current/bin/magicvault-mcp" --profile agent
codex mcp get magicvault
```

This follows the [official Codex MCP documentation](https://learn.chatgpt.com/docs/extend/mcp?surface=cli).
Record native pairing, enrollment, exact-origin permission and browser
registration before starting a fresh Codex session. Keep ordinary browser
automation available for navigation and submission. Leave security approval
and credential entry to the human.

Record the real tool calls, native use approval, filled fields, receipt,
submission and website result. A `filled` receipt alone does not prove login
success. Keep secret values out of command arguments, prompts, labels and tool
responses. Share only the intended windows and masked native input dialogs.

## Other cuts

The earlier [37.5-second MCP GIF](assets/magicvault-mcp-demo.gif) and
[MP4](assets/magicvault-mcp-demo.mp4) show a separate successful login take with
setup completed beforehand. Reproduce it using:

```bash
python3 scripts/render-live-mcp-demo.py --capture-root output/live-mcp-demo
```

The [illustrated overview](assets/magicvault-demo.gif) is a separate synthetic
explanation, explicitly labeled as an illustration. It also introduces
HTTP/process uses and the TypeScript client; those scenes are not live footage.

```bash
python3 scripts/render-demo.py
```
