# MagicVault documentation

Start with the [quick start](../README.md#quick-start) to use MagicVault, or
[coverage and limits](coverage.md) to check whether it supports your destination.
The standalone application is a macOS Apple Silicon source alpha; see
[installation availability](distribution.md#availability) and the
[release acceptance checklist](qualification/release.md) before using a candidate.

## Use MagicVault

| Task | Guide |
| --- | --- |
| Install a candidate, upgrade, diagnose or uninstall | [Installation and distribution](distribution.md) |
| Build from source or configure local MCP | [Setup](setup.md) |
| Fill credentials through CDP or a Chromium extension | [Browser usage](browser-usage.md) |
| Deliver credentials to a new process or HTTP request | [Process and HTTP usage](delivery-usage.md) |
| Use the CLI from an agent or script | [CLI recipes](cli-usage.md) |
| Choose per-use approval, remember an exact use, or revoke consent | [Consent](consent.md) |

## Understand and integrate

- [Security boundary and reporting](../SECURITY.md)
- [Architecture and reviewed drift baseline](architecture.md)
- [Local protocol and trust boundary](protocol.md)
- [Application, extension and library integrations](integrations.md)
- [Versions and compatibility](versioning.md)
- [Changelog](../CHANGELOG.md)

## Test and qualify

- [Test lanes, isolation, build paths and coverage](testing.md)
- [Acceptance runbooks and version-specific evidence](qualification/README.md)
- [Current release gates](qualification/release.md)

## Documentation scope

Keep user guides, technical contracts, architecture, troubleshooting, test
runbooks and reproducible qualification evidence here. Task plans, implementation
diaries, agent handoffs and commit/staging notes do not belong in public docs.
Summarize user-visible changes in the changelog, not each investigation step.

Retain dated evidence with its original source/version, failures and limitations;
link it from the qualification index instead of copying its history into guides.
An old pass is not qualification of a new binary. Update the release checklist
when evidence changes, without erasing failed trials or hiding remaining gates.
Never commit credentials, account/payment details, private signing material,
vault contents or raw diagnostic artifacts.
