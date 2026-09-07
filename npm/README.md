# MagicVault

**Let agents use credentials without seeing them**

Reference-only credential delivery for browser automation, new processes and
HTTP requests. A native daemon holds credentials, requests human approval and
delivers them to an authorized recipient; MCP/CLI clients receive closed status,
not credential values or raw recipient output.

Authorized recipients receive the secret; separate browser tools can still read
it afterward. [Security boundary](https://github.com/MagicBeansAI/MagicVault/blob/main/SECURITY.md).

**Start with MCP for Codex, Claude Code or another local agent.** The agent can
discover references, request `secure_fill`, `secure_new_process` or
`secure_new_http`, and poll status. Use the CLI for shell-based agents/scripts;
Rust crates and the local protocol are for developers embedding MagicVault.
No dedicated Python/Node SDK is shipped. This npm package launches native binaries;
it is not a JavaScript credential SDK or a hosted MCP service.

This is early-access software. Use synthetic credentials first. Packaging does
not certify secret isolation from unrestricted same-user software, recipient
websites/processes, or another browser tool reading the page afterward.

## Start with MCP

Requires macOS Apple Silicon, Node 22+, a logged-in desktop session and the
matching optional native package. No Rust toolchain is needed. Package
installation has no lifecycle hooks and does not start services or create a vault.

```bash
magicvault --version
magicvault --profile agent setup
magicvault --profile agent doctor
magicvault --profile agent enroll --label 'Demo account' --field password
```

Enter values only in the native hidden prompt. Setup installs into a private,
stable app directory separate from the credential vault and prints an absolute-path
`mcpServers` configuration without capability tokens. Prefer that stable MCP
command over a cache-dependent npx command. `magicvault-mcp` is also provided.

Next, follow the [Codex/Claude Code connection commands](https://github.com/MagicBeansAI/MagicVault#use-with-an-mcp-agent)
and authorize your browser or fixed process/HTTP destination. Your MCP client
launches the stdio bridge; the installed daemon stays responsible for custody.
Ask the agent to check `vault_status`, discover references and use the secure
tools. One-time human setup and native approval for every delivery still apply;
MCP does not grant itself access, enroll credentials or remove those prompts.

For explicit shell calls instead, see [CLI automation](https://github.com/MagicBeansAI/MagicVault#cli-for-agents-and-scripts).
Scripts use the same consent and receipt-only boundary, not unattended CI access.
For embedding, see [developer integrations](https://github.com/MagicBeansAI/MagicVault#build-on-magicvault).

After explicitly installing matching newer packages, run `magicvault upgrade`.
`magicvault uninstall` unloads owned integrations and archives app files while
preserving the vault, keychain and pairing files. npm uninstall is separate.

See the [quick start](https://github.com/MagicBeansAI/MagicVault#quick-start),
[installation and signing details](https://github.com/MagicBeansAI/MagicVault/blob/main/docs/distribution.md),
[destination coverage](https://github.com/MagicBeansAI/MagicVault#what-works-today)
and [security boundary](https://github.com/MagicBeansAI/MagicVault/blob/main/SECURITY.md).
Local candidate packages are not a claim of npm publication, Apple signing or
notarization; verify the publisher and release evidence before trusting a download.
