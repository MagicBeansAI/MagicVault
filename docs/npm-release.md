# npm releases

Pushing a version tag such as **`v0.9.0`** starts
[npm release](../.github/workflows/npm-release.yml): validate the source, build
six native packages, verify the complete set, then publish those packages and
the launcher/SDK under **`latest`**. Every push to `main` automatically runs the
source checks and six-platform package preparation without publishing. Manual
release-workflow runs also prepare artifacts only.

`latest` is npm's default install channel. Platform support remains **alpha**;
the channel does not change the [recorded platform limitations](platforms.md).
The workflow does not sign/notarize desktop executables or create a GitHub Release.
Publishing GitHub release notes for an existing tag does not trigger a second run.
[npm dist-tag behavior](https://docs.npmjs.com/cli/v11/commands/npm-publish/).

## One-time repository setup

1. The workflow's confirmed release scope is **`@magicbeansai`**, producing
   **`@magicbeansai/magicvault`** and its six native dependencies. It is configured
   in the versioned workflow; no repository variable is needed. Manual preparation
   can override the scope for that run. `@magicvault-local` is refused for releases.
2. Keep **`NPM_TOKEN`** in Actions secrets. It must authorize publication of
   all seven public names in that scope, including creating them on the first
   release. The workflow exposes it only to the publish step as `NODE_AUTH_TOKEN`.
   An expired token or one without unattended-publish permissions will fail.
3. Enable Actions and the hosted runners required by the release matrix. The
   tagged commit must already be on `main`. Restrict who may push release tags
   according to the repository's maintainer policy.

The repository owner's GitHub name does not establish npm scope ownership.
The configured token's presence alone does not prove npm publication rights.
Current token-based automation uses an appropriate granular token with bypass
2FA enabled. [npm CI authentication](https://docs.npmjs.com/using-private-packages-in-a-ci-cd-workflow/).
For longer-term credential-free releases, npm also supports
[trusted publishing](https://docs.npmjs.com/trusted-publishers/); this workflow
currently uses the existing `NPM_TOKEN` integration.

## Packages and checks

`@magicbeansai/magicvault` contains the Node/TypeScript client and CLI/MCP
launchers. Its six optional dependencies are pinned to the exact same version:

| Native package suffix | Hosted build runner |
| --- | --- |
| `darwin-arm64` | `macos-15` |
| `darwin-x64` | `macos-15-intel` |
| `linux-x64` | `ubuntu-22.04` |
| `linux-arm64` | `ubuntu-22.04-arm` |
| `win32-x64` | `windows-2022` |
| `win32-arm64` | `windows-11-arm` |

Each native package contains the CLI, MCP server, browser native host, desktop
prompt and browser assets. Node 22+ selects the matching dependency; installation
runs no lifecycle hooks. Linux packages require glibc 2.35 or newer; Alpine/musl
is not included. Windows governed process delivery remains unsupported.
[GitHub runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).

The release gates are:

- **Version and source:** `vX.Y.Z` must exactly match CLI, MCP, service and prompt
  manifests. Prerelease tags, mismatches and commits outside `main` are refused.
- **Source CI:** architecture drift, documentation links, Python checks,
  SDK/package/release tests, ESM/CommonJS TypeScript declarations, extension tests
  and Actions workflow linting. The same checks run for normal
  main pushes and pull requests. Desktop CI also checks macOS, Linux and Windows.
- **Native builds:** Rust 1.92, platform libraries/tests, all four executables,
  and execution of the packaged CLI/MCP version commands on all six runners.
- **Artifacts:** seven allowlisted npm tarballs with SHA-512 integrity, exact
  identity, native platform metadata and all six pinned dependencies.
- **Publication:** registry preflights complete before any mutation. Native
  packages publish first, the common launcher last. Explicit public access,
  `latest`, ignored lifecycle scripts and npm provenance are used. Registry
  integrity and latest tags are read back after publication.

The publish job alone receives the token and OIDC permission for
[npm provenance](https://docs.npmjs.com/generating-provenance-statements/).
No build/test job receives publishing credentials. Passing these checks is
distinct from native desktop/keychain/browser acceptance on every platform.

## Prepare without publishing

Push the source changes to `main`. The **npm release** workflow automatically
runs source checks, then builds and verifies all six platform artifacts. Its
publish job is skipped for branch pushes, so a separate preparation dispatch
is unnecessary.

To explicitly repeat preparation, select **Actions → npm release → Run workflow**
on `main`. Leave the scope input empty to use `@magicbeansai`, or supply an owned
scope for that preparation run. Manual dispatch cannot publish, even when run
against an existing version tag.

Review all six platform artifacts and `MagicVault-release-plan`. The generated
npm README contains real scoped install commands, the JIT GIF, narrated-video
link and SDK imports. Demo assets must be on GitHub before publishing so npm's
hosted image links resolve. Raw recordings, vaults and narration intermediates
are excluded from the npm package and Git.

## Release a version

Update the standalone component versions together, lockfile, changelog and
reviewed architecture baseline before tagging. Shared libraries and the browser
extension retain their independent versions. Commit and push `main`, review CI
and the automatic package-preparation results, then deliberately push the version tag:

```bash
# Example for the current source version; these commands publish when pushed.
git tag -a v0.9.0 -m 'MagicVault 0.9.0'
git push origin v0.9.0
```

The tag push performs the release; no second publish checkbox is required.
After it succeeds, these resolve through `latest`:

```bash
npm install --global @magicbeansai/magicvault
magicvault --profile agent setup
magicvault --profile agent doctor
```

For applications use `npm install @magicbeansai/magicvault`. Update the project
README's availability wording only after the package has actually been published.

## Recover a failed release

Publication stops on an uncertain upload result; it does not repeat the upload.
Inspect npm, then **rerun the failed publish job in the same Actions run**, using
the retained artifacts. Identical published versions are skipped, and an older
or absent latest tag can be repaired. Different bytes or a newer latest version
abort before any mutation, so an old release cannot roll back the default install.

Do not rerun all build jobs to recover a partially published version: a rebuild
may produce different bytes. Published versions are immutable. Artifacts are
retained for 14 days; if they are unavailable, prepare a new source version.
Concurrent release runs are serialized within this repository, but npm does not
provide an atomic transaction across seven packages. Avoid publishing the same
scope/packages independently while this workflow is running.

## Current readiness

Local pre-push validation passed: 122 JavaScript tests, 26 Python tests,
ESM/CommonJS declaration checks with TypeScript 5.9.3, workflow linting with
actionlint 1.7.12, the architecture gate and local links in 56 documentation files.
The loopback-server fixture was rerun outside the sandbox after its port bind
was blocked. Release publication tests use a simulated registry; they do not
prove the live token or npm permissions.

This workflow is prepared locally. Its complete six-runner build and publication
have not yet run for 0.9.0. The latest historical GitHub qualification and
unsigned-distribution passes were for `81fe8c6` (0.8.3), not these changes.
The release scope is configured as `@magicbeansai`. A valid publishing token and
a successful automatic preparation run are required before the first release
tag is pushed.
