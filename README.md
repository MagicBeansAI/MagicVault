# MagicVault

The easiest way to keep credentials away from models.

This repository currently contains the **Phase 1 shared libraries**, not the
standalone browser/HTTP/process product. CLI, MCP, `secure_fill`, extension/native
messaging, `secure_new_http`, and `secure_new_process` are later milestones.

## Libraries and trust boundary

- `magicvault-core`: encrypted provisioned/captured/OAuth partitions, ephemeral
  references, policy/approvals/grants, captured-session routing, redaction, and a
  scoped store cache parameterized by the host's path layout.
- `magicvault-primitives`: stack-safe JSON and durable filesystem primitives,
  separately consumable without a credential backend or product runtime.

Neither crate depends on Magician or MagicRun. Trusted applications provide a
`MasterKeyProvider` and, for shared scoped resolution, `SecretScopeLayout`.
Applications can append their typed metadata-only audit receipt by implementing
`AuditReceipt`; unstructured strings/JSON do not implement that contract.
The core's ordinary events have no product receipt.

These are trusted in-process APIs: a consumer can receive plaintext. This does
not promise that privileged local software, an authorized recipient, or arbitrary
model-authored code can never recover material. The model-facing contract belongs
to each integration: reference-only requests, destination authorization, approval,
material delivery, and explicitly covered output filtering. No raw credential-read
tool is registered by these libraries.

The first extracted consumer, Magician, retains OS-keychain service/account names,
app-data keys and signing, startup policy, runtime directories, action traversal,
brokers, analytics, browser and process ownership. Its historical module paths
re-export these types. No daemon, second store, key rotation, or vault migration
is introduced by the extraction.

## Evidence and development

The source baseline is Magician `aef928c000138fea035eea034118f8c23db582ed`.
Existing custody, policy, crypto, redaction and JSON tests moved with their code.
Product action/result and session integration tests remain with Magician.
Additional synthetic tests characterize old/new vault framing and JSON formats,
typed audit serialization, durable publication, and the consumer facade.

Run `make check` / `make test` only when verification is permitted. **No checks or
tests were run during this extraction**, by explicit owner instruction. Compile,
runtime, performance, coverage, recovery, and platform results are unverified.
The workflow is manual-only and has not been dispatched. Static review does not
certify these results or authorize deploying over a live credential store.

Existing source-attested app packages can require their normal trusted re-review
after a dependency/source change. Consumers must hash actual compiled upstream
source, never freeze an old digest or bypass an identity mismatch for an upgrade.

Only curated library source and synthetic tests are exported, without Magician
history, live configuration, runtime data, or credentials. Repository visibility
is unchanged. Licensed under MIT OR Apache-2.0.
