# CLI and daemon acceptance runbook

## What is already automated

- `make test-foundation`: actual CLI and official SDK MCP subprocesses, real
  Unix IPC, pairing/enrollment, closed output, malformed frames, bounded encoder
  and stdio teardown. The custody key and human interaction are synthetic.
- `make test-browser`: reference-only browser flows, native-host subprocess,
  audit/lifecycle, malformed transport and uncertainty behavior.
- `make test-cli-native`: actual CLI → real daemon → actual disposable headless
  Chrome, including approval/denial, DOM verification and value-free output/audit.

None of these proves that a real human sees an untruncated consent dialog or
that a macOS keychain/LaunchAgent installation works. Use the following manual
gate in a disposable OS account; do not add an auto-approve flag to production.

## Genuine native path

1. Build with `make build-standalone`. Record the exact source/toolchain and set
   `magicvault_qa_bin` to the trusted absolute path of the resulting executable.
2. Create a fresh private root, then initialize and start it:

   ```sh
   vault_qa_root=$(mktemp -d /tmp/mv-qa.XXXXXX)
   "$magicvault_qa_bin" --root "$vault_qa_root" init
   "$magicvault_qa_bin" --root "$vault_qa_root" serve
   ```

   This creates a test keychain identity. It is not a read-only step. Leave the
   daemon in its terminal and use the same root in a second terminal.
3. Pair and enroll synthetic data only:

   ```sh
   "$magicvault_qa_bin" --root "$vault_qa_root" --profile qa pair --label 'Qualification only'
   "$magicvault_qa_bin" --root "$vault_qa_root" --profile qa enroll --label 'Synthetic login' --field username --field password
   "$magicvault_qa_bin" --root "$vault_qa_root" --profile qa list-credentials
   ```

   Enter synthetic values in native hidden prompts, never command arguments.
   Verify default Deny/Cancel, correct caller/ref/field text and no value output.
4. Start the [fixture site](browser.md), connect a disposable CDP browser or follow
   [extension setup](extension.md), and configure browser permissions for profile
   `qa` using the **same explicit root**. A successful metadata request must not
   make the credential fillable without browser configuration and per-use consent.
5. Follow [reference-only fill/status/cancel](../browser-usage.md#request-a-fill-and-inspect-its-outcome)
   with actual handles and a fresh operation UUID. Check the cases below.

| Case | Required observation |
| --- | --- |
| C-02 genuine consent | Native UI names exact caller, top/frame origin and quoted locators; approve only the intended request |
| C-03 deny/cancel/expiry | No recipient write before approval; final status is not success; no raw-value fallback |
| C-04 navigate during consent | Old target is refused; replacement document remains empty |
| C-05 restart/lost reply | Old handles become invalid; missing status is not interpreted as safe retry |
| C-06 output/audit | Own stdout/stderr and typed receipts omit the synthetic value; no clipboard or argv credential path |
| C-07 native rendering | Long valid permission/selector prompts are fully inspectable, not clipped into misleading consent |
| C-08 ownership/shutdown | SIGINT/SIGTERM drains this daemon; original browser automation continues; no other store or service is replaced |

Do not simulate power-loss durability by merely killing a client; that is a
separate crash/recovery gate. Use generated test roots only for fault injection.
Stop this foreground daemon explicitly when done. Native test key/installation
cleanup belongs to the disposable OS account or deliberate operator cleanup;
do not delete a vault/key automatically or use an existing production root.
