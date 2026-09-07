# Installation and distribution

MagicVault has a native Rust application and a thin npm launcher. MCP is a stdio
protocol, not a requirement to implement custody in JavaScript. The npm package
exposes `magicvault` and `magicvault-mcp`; its exact-version optional dependency
contains the three compiled executables, unpacked extension, reference-only
examples and licenses. There are no installation scripts, runtime downloads,
Rust compilation, shell-command construction or credential handling in Node.

## Availability

| Surface | Implemented distribution | Current limit |
| --- | --- | --- |
| CLI and MCP | Local npm tarballs with prebuilt native executables | macOS Apple Silicon; Node 22+; not published to npm yet |
| Daemon | Explicit native `setup`, private stable install and user LaunchAgent | Interactive macOS desktop for consent/keychain; not started by npm/MCP |
| Chromium extension and native host | Assets bundled; `extension install` finds the installed host | Load unpacked manually; Chrome/Chromium only; no Web Store listing |
| Rust embedders | Existing core/primitives and standalone crate sources | Core API/format unchanged; no dependency on npm or managed setup |
| Intel macOS, Linux, Windows prebuilt packages | Not shipped | Additional native custody/UI/service backends and qualification required |
| Apple-verified release, registry provenance | Release procedure provided | No signing, notarization or publication performed by this change |

The candidate scope `@magicvault-local` is for **local tarballs only**. A maintainer
must select and verify ownership of a public npm scope before publishing. Do not
tell users to install an unverified package name or use `@latest` in a reviewed
MCP configuration. After publication, the same launcher supports npm/npx; setup's
stable MCP path avoids relying on npx's cache or fetching code at client startup.

## Setup and lifecycle

Install both matching tarballs as shown in the [quick start](../README.md#quick-start).
Then run the human-facing commands, using the same `--root`, `--app-dir` and
`--profile` choices on subsequent invocations:

```bash
magicvault --profile agent setup
magicvault --profile agent doctor
# Copy just application files, with no keychain, vault, service or pairing:
magicvault setup --install-only
```

Defaults are `~/.magicvault-app` for application files and `~/.magicvault` for
custody. Custom parents must already exist; roots must not overlap. Vault paths
remain private, absolute and at most 85 bytes for the Unix socket. Do not run as
root/sudo, point at Magician's data, or put credentials in paths, labels or profile
names. No setup operation grants browser/process/HTTP delivery authority.

Setup returns a `mcpServers` object with the installed MCP executable's absolute
path and reference-only root/profile arguments. Use that configuration in your
MCP client. Existing CLI commands and manually run daemons remain supported.
`doctor` does not initialize custody or read keychain material; inspect its
`installation` and `daemon` fields, not merely its exit code. Its file-integrity
check is **not** publisher-signature verification.

For the extension, load the printed `extension_directory` in `chrome://extensions`
using Developer mode → Load unpacked. Copy its extension ID, then:

```bash
magicvault --profile agent extension install --extension-id REPLACE_WITH_EXTENSION_ID
```

Grant the exact sites in the extension's setup page, click Connect and approve
the native prompt. Native definitions are OS-user-wide; another installation's
definitions are never overwritten. [Full extension instructions](browser-usage.md#chromium-extension).

### Upgrade

Install the two matching new npm tarballs, then run the **new** CLI:

```bash
magicvault upgrade
magicvault --profile agent doctor
```

The npm update alone leaves the daemon untouched. Explicit upgrade verifies and
stages a complete immutable bundle, checks the exact owned LaunchAgent, unloads
it when loaded, waits for the daemon's single-writer lease, then atomically switches
the private `current` symlink. The same stable daemon/MCP/native-host paths select
the new version. A previously loaded service is restarted and readiness checked.
An application-only installation can upgrade without creating custody. Downgrades
are refused. Old complete bundles are retained, not automatically pruned.

Reconnect MCP clients, reload the unpacked extension and reconnect browser handles.
Check that Chrome actually loaded the current assets, not a retained resolved
version-directory path. If necessary, remove and load unpacked again from the
printed current directory. Check the extension ID afterward; if it changed,
remove this instance's native definitions with `extension remove`, then reinstall
with the new exact ID and approve the connection again. Extension identity is
not promised to survive every unpacked-directory/browser installation workflow.
Restart invalidates pending operations/handles; missing status never permits
automatic replay. Registry permissions, vault bytes and keychain identities are
not migrated by application installation. Existing service definitions pointing
at manual build paths cause a conflict, not silent takeover: stop/remove those
owned definitions with the existing low-level commands before managed setup.

### Uninstall and interrupted operations

```bash
magicvault uninstall
```

Uninstall unloads an exact owned service, waits for its lease, removes only matching
native/LaunchAgent definitions, and renames the private app directory to a unique
`.uninstalled-UUID` sibling. It **preserves vaults, keychain identities and pairing
files**. The app archive remains recoverable. Remove the extension in Chrome,
the MCP configuration and the npm launcher separately. This is not credential
revocation or secure erasure; use the actual provider to revoke an exposed secret.

Unknown owners, changed definitions, symlinked assets, modified bytes, a busy
writer or partial identity/pairing files fail closed. No forced kill, automatic
rollback, vault deletion or key regeneration is attempted. A failure after
activation can leave the new version installed but stopped; run `doctor`, inspect
the retained application data and use `setup` to resume a verified installation.
A crash before an ownership/completion marker can require explicit operator
repair; do not delete unknown directories to make setup succeed. Uninstall errors
can leave some integrations removed; they do not claim a successful rollback.

## Build local candidates

Maintainers need the [Rust toolchain](../README.md#rust-toolchain), Node 22+ and
macOS arm64. The scope is explicit; assembly never publishes:

```bash
export CARGO_TARGET_DIR="$(make -s print-target-dir)"
make build-standalone
candidate_dir=$(mktemp -d /tmp/magicvault-candidate.XXXXXX)
make package-npm NPM_SCOPE=@magicvault-local PACKAGE_OUTPUT="$candidate_dir/packages"
make test-package-install PACKAGE_OUTPUT="$candidate_dir/packages" \
  PACKAGE_TEST_OUTPUT="$candidate_dir/qualification"
```

The tarballs are in the qualification directory. Use an SSD1 directory instead
of `/tmp` when desired; Rust output continues to follow `CARGO_TARGET_DIR`.
The manual **Unsigned distribution candidate** workflow runs compilation, tests,
assembly and packaged-install qualification, then uploads only the two tarballs.
It has read-only repository permissions and no publication/signing credentials.
Workflow execution is distinct from adding the workflow to the source tree.

## What gets signed?

Sign **`magicvault`, `magicvault-mcp` and `magicvault-native-host`**, the compiled
Mach-O executables. The CLI binary also runs the daemon. Do not sign Rust source
or a Cargo crate as if that established executable identity. MagicRun is linked
into the native application; users do not install or sign it separately.

Apple Developer ID signing identifies the publisher of executable bytes.
Notarization submits signed code for Apple's automated checks; it is not proof
of credential isolation or absence of bugs. npm provenance separately links an
npm publication to its source/build workflow; it is not an Apple code signature.
Bundle SHA-256 checks detect corruption and mismatched files, **not authenticity
against an attacker who can replace both a binary and its manifest**.
Do not assume npm installation invokes Gatekeeper or verifies Apple identity:
the launcher checks hashes, not certificates, and `doctor` reports that distinction.

With separately authorized Developer ID credentials and a preconfigured
`notarytool` keychain profile, the maintainer may explicitly run:

```bash
export MAGICVAULT_SIGNING_IDENTITY='Developer ID Application: YOUR VERIFIED IDENTITY'
export MAGICVAULT_NOTARY_PROFILE='YOUR PRECONFIGURED NOTARY PROFILE'
sh scripts/sign-release.sh "$CARGO_TARGET_DIR/release"
# Only after Accepted: assemble fresh packages from those exact signed bytes.
```

The script signs all three executables with hardened runtime and a timestamp,
verifies their signatures, and notarizes an archive. It modifies the supplied
release binaries and retains the private notarization result outside the repo.
Do not commit certificates, private keys, passwords or signing logs. Never sign
unreviewed pull-request artifacts with release credentials. Bare executables and
ZIP archives cannot be stapled; Gatekeeper retrieves notarization tickets online.
See Apple's [notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow).

Before public distribution, verify Developer ID identity and Gatekeeper behavior
on a fresh/quarantined download, rerun qualification against the exact signed
packages, and complete native keychain/LaunchAgent/extension acceptance in a
disposable OS account. No such native acceptance or signed-release claim follows
from unsigned local tests.

For registry release, configure an owned npm scope and a protected trusted
publisher workflow, publish the native package first and the matching launcher
second, and verify registry provenance/install results. That publishing workflow
and its external account authority are intentionally **not enabled** by the
candidate workflow. Review npm's [trusted publishing](https://docs.npmjs.com/trusted-publishers/)
and [package platform/bin metadata](https://docs.npmjs.com/cli/v11/configuring-npm/package-json/).
