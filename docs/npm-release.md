# npm releases

A matching **`vX.Y.Z` tag push** builds, verifies and publishes one package:
**`@magicbeansai/magicvault`**, under npm's **`latest`** channel. From 0.9.1 the
package bundles all six native builds. There are no platform-package dependencies,
install scripts or runtime binary downloads. Users install only the main package.

Main pushes and manual workflow runs prepare artifacts without publication.
Platform support remains **alpha**. Publication does not sign/notarize desktop
executables or create a GitHub Release. See [platform limits](platforms.md).

## Repository setup

- The confirmed scope is `@magicbeansai`, configured in the versioned workflow.
  Manual preparation can override it; `@magicvault-local` is refused for releases.
- Keep `NPM_TOKEN` in Actions secrets. It must authorize public publication in
  this scope, provenance and unattended publishing. The 0.9.1 migration also
  requires permission to deprecate the existing packages. Only the publish job
  receives it as `NODE_AUTH_TOKEN`. Package deletion requires interactive
  authentication; a bypass-2FA automation token cannot unpublish packages.
- A matching release tag must point to a commit on `main`. A source/version
  mismatch, prerelease tag or commit outside main is refused.

Token-based automation needs appropriate granular-token permissions and bypass
2FA. [npm CI authentication](https://docs.npmjs.com/using-private-packages-in-a-ci-cd-workflow/).
The current workflow uses `NPM_TOKEN`; [trusted publishing](https://docs.npmjs.com/trusted-publishers/)
is a separate future option.

## Build and verification

The workflow builds the CLI/daemon, MCP server, browser native host and desktop
prompt on native runners using Rust 1.92:

| Bundled platform | Runner |
| --- | --- |
| `darwin-arm64` | `macos-15` |
| `darwin-x64` | `macos-15-intel` |
| `linux-x64` | `ubuntu-22.04` |
| `linux-arm64` | `ubuntu-22.04-arm` |
| `win32-x64` | `windows-2022` |
| `win32-arm64` | `windows-11-arm` |

Each runner produces a **private candidate**, not a publishable native package.
The assembly job validates their exact file lists, SHA-512, metadata, native
architecture and per-file SHA-256 manifest. Common launcher/README bytes must
agree across candidates, and bundled assets must match the release source.
It copies only allowlisted file contents into a fresh universal package.

The final package contains `native/<os>-<cpu>` directories and no dependency or
lifecycle-hook fields. npm OS/CPU constraints reject unsupported combinations;
the launcher selects the matching bundled executable and checks its version/hash.
Node 22+ is required. Linux needs glibc 2.35+; Alpine/musl is not covered. Windows
governed process delivery remains unsupported. All builds contribute to download
size, even though only the matching platform runs.

Before publication, **all six runners install the exact final tarball offline,
with `--ignore-scripts --omit=optional`**. Each confirms that only one package was
installed, then executes CLI/MCP version checks and CommonJS/ESM SDK imports.
These gates supplement source checks, native library tests, documentation links,
architecture/durability gates, TypeScript checks and workflow linting.
They do not replace interactive desktop/keychain/browser acceptance.

## Publish a version

Update the CLI/MCP/service/prompt versions together, lockfile, changelog and
reviewed architecture baseline. Commit source changes to main. The release tag
is the publication decision; its workflow gates publication on all checks:

```bash
# Example for the current source version; pushing this tag publishes after checks.
git tag -a v0.9.2 -m 'MagicVault 0.9.2'
git push origin v0.9.2
```

The publish job verifies the single artifact and registry state, refuses differing
bytes or a newer `latest`, and publishes with explicit public access, `latest`,
ignored lifecycle scripts and [npm provenance](https://docs.npmjs.com/generating-provenance-statements/).
It then reads back package integrity and the tag, with bounded read-only retries
covering npm's five-minute metadata cache lifetime. No upload is automatically retried after uncertainty.

Main pushes and **Actions → npm release → Run workflow** prepare and test the
same artifact without publishing. Manual dispatch cannot publish even on a tag.

## Migrate and retire the 0.9.0 packages

Users update **only the main package**, then explicitly upgrade the managed app:

```bash
npm install --global @magicbeansai/magicvault@latest
magicvault upgrade
```

For projects, omit `--global` and use `npx magicvault upgrade`. Preserve any
custom root/app/profile options. npm removes obsolete transitive dependencies;
explicitly installed native packages can be removed from the project's direct
dependencies after upgrading. Custody, pairing and OS keychain locations do not
change. npm installation never changes the running daemon or vault itself.

After verified 0.9.1 publication, the workflow's one-time migration:

1. Confirms `latest` is 0.9.1 and its metadata describes a self-contained bundle.
2. Confirms the six known native packages contain only 0.9.0.
3. Deprecates those entire packages and main version 0.9.0, with an upgrade message.
4. Reads back each deprecation; already-matching messages are skipped on reruns.

That first migration **deprecated packages without deleting them**; their direct
URLs remained accessible. No future version publishes native packages.

The original 0.9.2 tag published successfully, but npm rejected its separate
cleanup job with 403. npm's current
[authentication rules](https://api-docs.npmjs.com/) reject package deletion using
bypass-2FA granular tokens, even when the same token can publish and deprecate.
The unsupported CI cleanup jobs were removed from main. Their failure did not
roll back the published description or delete any package.

Run the bounded cleanup from an interactive maintainer terminal after login:

```bash
npm login --auth-type=web --registry=https://registry.npmjs.org/
node scripts/release-npm.mjs --mode remove-legacy --scope @magicbeansai --version 0.9.2 --interactive
```

Complete npm's authentication prompts directly; do not put passwords or OTPs in
chat, workflow inputs or committed files. The cleanup requires latest 0.9.2 and
the preserved 0.9.1 version. It checks bundled metadata and integrity, then
preflights all six known native package names. Each must already be missing or
contain only deprecated 0.9.0. It never unpublishes any main package version;
0.9.1 and latest 0.9.2 are preserved
with unchanged integrity. Reruns skip versions already removed.

Unpublishing is irreversible and fresh pinned 0.9.0 installs may no longer work;
upgrade to the self-contained main package first. npm's
[unpublish policy](https://docs.npmjs.com/policies/unpublish/) may refuse packages
with dependents or require
additional authentication. Cleanup stops on refusal or an uncertain write and
reports failure; deprecation is not reported as removal. Inspect registry state
before rerunning cleanup. It never retries a deletion automatically. Do not run
publication concurrently. npm's generic 403 detail mentions dependencies for
many failures; use the actual registry summary to diagnose the cause.

## Recover a failed release

Inspect npm, then **rerun only the failed publish job in the same Actions run**.
The retained universal artifact is reused; identical published bytes are skipped,
and retirement resumes safely. Do not rebuild a partially published version:
rebuilding may produce different bytes, and npm versions are immutable.

Artifacts are retained for 14 days. If they are unavailable, prepare a new source
version. Publication is serialized by the `magicvault-npm-release` concurrency
group; avoid independent publication of the same package while it runs.

## Release evidence

Version 0.9.2 was published as `latest` from tag `v0.9.2`, commit
`34c1b336790287d27beafe1d9703b4cf6f1aeddd`. Its
[release run](https://github.com/MagicBeansAI/MagicVault/actions/runs/35131736336)
passed source checks, all six builds, universal artifact verification, all six
offline installation checks and publication. A fresh npm install added exactly
one package; CLI/MCP versions and CommonJS/ESM SDK imports passed. The public
registry description now explains MCP, CLI, TypeScript, encrypted storage,
private prompts and just-in-time login. The subsequent cleanup jobs failed with
403; those failures did not undo publication. Deletion requires the separate
interactive authentication described above.

Version 0.9.0 was published on 2026-09-16 from `v0.9.0`, commit
`ea92d72b06ad6dc92fd00d67fd5ce60f3f3bd644`, using the original seven-package layout.
Its [release run](https://github.com/MagicBeansAI/MagicVault/actions/runs/35120650287)
passed all six builds and artifact checks. An immediate registry read-back failed;
rerunning the publish job verified all existing bytes and `latest` tags without
uploading again. A fresh macOS ARM64 install passed CLI/MCP/SDK checks.

Version **0.9.1** was published on 2026-09-16 from `v0.9.1`, commit
`89458078916ff502250e6144494c752040b3e28d`. Its
[release run](https://github.com/MagicBeansAI/MagicVault/actions/runs/35126703108)
passed all six native builds, universal artifact verification and all six offline
single-package CLI/MCP/SDK install checks. Source CI and all three desktop-platform
jobs also passed. The duplicate main-branch preparation was deliberately cancelled
so the fully gated tag release could run without waiting for a redundant build.

The first upload succeeded, but its initial 31-second read-back window was shorter
than npm's observed `max-age=300` metadata cache. Retrying only the publish job
verified the same bytes without uploading again and completed all seven legacy
deprecations. The publisher now allows a full cache lifetime for read-only checks.

A fresh macOS ARM64 registry install with scripts and optional dependencies disabled
installed exactly one package. CLI/MCP reported 0.9.1 and SDK imports passed.
Updating a disposable 0.9.0 install removed its old native dependency automatically.
The published package contains all six platforms and has no dependency or lifecycle
script fields. Old 0.9.0 tarballs remain available for compatibility.
