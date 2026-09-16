# npm alpha launch

The launch workflow is prepared in `.github/workflows/npm-alpha.yml`. Nothing
has been published by adding it. macOS, Linux and Windows remain **alpha**;
[platform evidence](platforms.md#verification) lists the native acceptance still
outstanding. Windows governed process delivery is not supported.

## Package set

One launcher/SDK package, `@YOUR_SCOPE/magicvault`, depends on six exact-version
native packages: `magicvault-darwin-arm64`, `magicvault-darwin-x64`,
`magicvault-linux-arm64`, `magicvault-linux-x64`, `magicvault-win32-arm64` and
`magicvault-win32-x64` under that same scope. Each native package includes the CLI,
MCP server, native browser host, private prompt executable and browser assets.
npm on Node 22+ selects the appropriate optional dependency; installation runs no hooks.

The initial source/package version is `0.9.0`. Publication explicitly uses the
**`alpha` dist-tag**, and generated `publishConfig` also defaults to `alpha`.
The release does not update `latest`. The binaries and bundle retain matching
numeric versions for installer compatibility. [npm tag behavior](https://docs.npmjs.com/cli/v11/commands/npm-publish/).

The workflow builds natively on all six OS/CPU runners, executes library tests
and the packaged CLI/MCP `--version` commands, and packs local tarballs. Linux
binaries use Ubuntu 22.04 (glibc 2.35 baseline); Alpine/musl is not a target.
Runner labels follow [GitHub's hosted-runner table](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
These build checks do not replace logged-in desktop, keyring or browser acceptance.

## Prepare and review

1. Choose an npm scope that you own. `@magicvault-local` remains a local-fixture
   scope and is refused by the release workflow. A GitHub organization name
   does not establish npm scope ownership.
2. After the local changes have been reviewed and deliberately pushed, select
   **Actions → npm alpha release → Run workflow**, choose `main`, enter the
   scope and leave **publish unchecked**. This produces six platform artifacts
   and a seven-package release plan. No publishing token reaches build or
   verification jobs.
3. Review all six runner results, package filenames/integrities, the generated
   scope-specific npm README, platform acceptance evidence and unsigned-binary
   limitations. No Apple Developer ID/notarization or Windows code signing is
   performed by this workflow.

The source npm README keeps local-candidate instructions. Release assembly uses
`--registry-readme` to replace its install section with real scoped `@alpha`
commands and updates SDK imports. The GitHub README does not claim an available
registry install until publication succeeds.

## Publish when authorized

Run the same workflow on the reviewed `main` revision with **publish checked**.
It rebuilds and verifies the complete set before publishing; this is an explicit
release operation, never a consequence of a push, tag or pull request.

The publish job passes the existing repository secret **`NPM_TOKEN`** as
`NODE_AUTH_TOKEN`, using `actions/setup-node`'s npm registry configuration.
The token must be valid and authorize publishing all seven public package names
in the selected scope. Token values are never embedded in repository files or
uploaded artifacts. Current npm token-based automation requires an appropriate
granular token and unattended-publish permissions. [GitHub configuration](https://docs.github.com/en/actions/tutorials/publish-packages/publish-nodejs-packages),
[npm token requirements](https://docs.npmjs.com/using-private-packages-in-a-ci-cd-workflow/).

Publication checks every tarball's SHA-512, package identity, platform, exact
dependencies, file allowlist and absence of lifecycle scripts. Registry
preflights for all seven finish before any upload. Native packages publish first;
the launcher publishes last. Public access, `alpha`, ignored lifecycle scripts
and npm provenance are explicit. The publish job alone requests OIDC permission
for provenance; it does not modify GitHub branches, tags or releases.

If an upload fails, the script stops without retrying an uncertain publication.
Re-run the failed job using the **same retained build artifacts** after inspecting
npm state. Already-published identical tarballs are skipped only when their alpha
tag matches. Different bytes, another tag or ambiguous registry responses refuse
the run; do not overwrite a version or silently rebuild it for recovery. Published
versions are immutable. The artifact retention window is 14 days.

After a successful launch, verify installs on the target desktops, update the
GitHub README's registry availability wording and use the actual owned scope:

```text
npm install --global @YOUR_SCOPE/magicvault@alpha
magicvault --profile agent setup
magicvault --profile agent doctor
```

For applications use `npm install @YOUR_SCOPE/magicvault@alpha` and the exported
Node/TypeScript client. The platform badges remain alpha until the recorded
release criteria justify changing them.
