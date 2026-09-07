# MagicVault

**Keep secrete away from Agents**

Reference-only credential delivery for browser automation, new processes and
HTTP requests. A native daemon holds credentials, requests human approval and
delivers them to an authorized recipient; CLI/MCP clients receive closed status,
not credential values or raw recipient output.

This is early-access software. Use synthetic credentials first. Packaging does
not certify secret isolation from unrestricted same-user software, recipient
websites/processes, or another browser tool reading the page afterward.

## Start

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

After explicitly installing matching newer packages, run `magicvault upgrade`.
`magicvault uninstall` unloads owned integrations and archives app files while
preserving the vault, keychain and pairing files. npm uninstall is separate.

See the [quick start](https://github.com/MagicBeansAI/MagicVault#quick-start),
[installation and signing details](https://github.com/MagicBeansAI/MagicVault/blob/main/docs/distribution.md),
[destination coverage](https://github.com/MagicBeansAI/MagicVault#what-works-today)
and [security boundary](https://github.com/MagicBeansAI/MagicVault/blob/main/SECURITY.md).
Local candidate packages are not a claim of npm publication, Apple signing or
notarization; verify the publisher and release evidence before trusting a download.
