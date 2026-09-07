# Browser integration runbook

## Automated local qualification

Choose a trusted, absolute Chrome/Chromium executable. On macOS Chrome, for example:

```sh
export MAGICVAULT_CHROME='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
make test-browser-native
make test-cli-native
```

The first command runs five explicitly ignored real-browser tests. The second
runs the shipped CLI through a real daemon and CDP browser, using synthetic
custody keys and human interaction. Both launch fresh browser processes/profiles
and a loopback site, then kill/reap only their own child. They do not install
extensions, touch a keychain or use an existing debugging endpoint. Headed tests
need a graphical session. Do not disable TLS or browser sandboxing to force a pass.

| Case | Required observation |
| --- | --- |
| B-01 headed fill | Username/password delivery, native input events, no submit, value-free outcome |
| B-02 headless fill | Same contract in modern headless Chrome |
| B-03 refusal/partial | Readonly, inherited disabled, hidden/file/short/ambiguous/missing/shadow targets refuse before writes; replacing a later field reports partial delivery |
| B-04 navigation | An old document binding cannot fill the replacement page |
| B-05 frames | Fill the explicitly selected same-origin frame only; opaque frame is omitted |
| C-01 real CLI | Pair/enroll/configure/discover/fill/status/disconnect via the executable; allow/deny behavior and output/audit canaries; original browser peer still works |

Ordinary input handlers are exercised, not every framework's event model.
Out-of-process cross-origin frames remain outside the initial CDP adapter's
guarantee. The extension's document-targeted frame path has its own manual gates.

## Reusable pages for manual and extension testing

```sh
node scripts/serve-browser-fixtures.mjs --port 8765
```

Or use `make fixture-site` for an ephemeral port; the server prints its loopback
origin. Use the exact printed scheme/host/port in MagicVault policy. Stop that
foreground server with Ctrl-C when finished.

| Path / selector | Purpose |
| --- | --- |
| `/login`, `#username`, `#password` | Ordinary and reactive inputs; a fixed fake-canary comparison and event counts only |
| `/controls` | Refusal cases: `#readonly`, `#fieldset-disabled`, `#hidden`, `#file`, `#short`, `.duplicate`, `#missing`, `#shadow-input` |
| `/controls`, `#replace-first` then `#replace-later` | Earlier input handler replaces a later control; no value goes to the replacement |
| `/frames` | `#same-origin-frame` plus `#opaque-frame`; select explicit backend frame handles, not guessed snapshot IDs |
| `/next` | Fresh same-origin document for navigation/consent race checks |

The fixtures live under `test-support/browser-fixtures/`. Rust real-browser
tests and the Node manual server serve the same HTML/JS. No external assets,
network callbacks or credential echo are used. CSP disables form transmission;
the synthetic submit button records only a local count. The server refuses POST,
unknown/query-bearing paths and does not log request metadata or bodies.

Use `SYNTHETIC-CHROMIUM-FILL-CANARY` only when testing the page's positive canary
indicator. That literal is public test data, not a credential. Never enter live
values or capture a profile containing them. A page showing a masked password
does not demonstrate observation filtering by another browser tool.

## Optional public-page compatibility smoke

```sh
MAGICVAULT_PUBLIC_WEB=1 make test-public-web
```

This separately ignored test visits [Example Domain](https://example.com/) and
[Selenium's demonstration form](https://www.selenium.dev/selenium/web/web-form.html)
in another disposable headless browser. It checks the documentation page, then
uses MagicVault's real CDP adapter to fill the demo password field with the fixed
synthetic canary. It blocks form submission locally and never attempts a login.

These sites can change, rate-limit or be unavailable. Record a failure or blocked
result; do not weaken origin checks, disable certificate verification, change to
a real login site or repeatedly retry to hide it. This is an optional low-volume
compatibility smoke, not a required offline CI gate or third-party endorsement.

## Failure and evidence handling

Native tests fail if `MAGICVAULT_CHROME` is absent; they do not guess a personal
profile. Socket denial is an environment blocker, not a passing transport case.
Retain only closed case outcomes, versions and sanitized diagnostics. If visual
inspection is needed, use a separate isolated automation session and ignored
`output/playwright/`; never point screenshot/trace tooling at a live vault session.
