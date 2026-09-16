# Capability coverage

The destination matters as much as the interface. Browser fills attach to an
existing session; process and HTTP operations create a **new, fixed-profile**
invocation. They do not offer arbitrary secret-bearing commands or URLs.

| Destination / use case | Available today? | Connection, conditions and limits |
| :--- | :--- | :--- |
| **One-time browser login without saved credentials** | **Yes — `secure_prompt_fill` (source candidate)** | Connected CDP/extension browser, native hidden input and final Use once. No enrollment or remembered grant. Automated coverage; new native UI acceptance pending. |
| **Existing browser/session (stateful), headed or modern headless** | **Yes — direct CDP fills** | Chrome/Chromium must expose a supported **loopback browser debugging websocket**. MagicVault opens a second connection; no extension or proxy is needed. The existing tool keeps the browser/session. |
| **Already-running headed browser without CDP** | **Yes — extension; basic native acceptance on one host** | Install the MagicVault Chromium extension **and native host**, run the daemon, and grant the target sites. No debugging port needed. Native keychain/allow/deny delivery qualified on one macOS/Chrome setup; broader recovery/permission cases remain open. |
| **Browser accessible only through a driver's private pipe, or a remote CDP endpoint** | **Not directly** | Configure an accessible local browser websocket, or use the headed extension path where installation is possible. An arbitrary browser/driver cannot be attached automatically. |
| **Embedded browser frames** | **Conditional** | CDP supports frames addressable through the attached page session. The extension uses explicit document/frame targeting and site permissions. Both top-page and frame origins must be allowed; opaque origins are refused. Broader cross-process frame compatibility is not claimed. |
| **New HTTP(S) requests** — `GET`, `HEAD`, `POST`, `PUT`, `PATCH`, `DELETE`, `OPTIONS`, `TRACE` and supported custom methods | **Yes — `secure_new_http`** | Human-registered exact URL/method and header/query/text/form/flat-JSON credential placements. Public HTTPS; plain HTTP only to explicit loopback IPs. No CONNECT, redirects, ambient proxy, retries, custom CA or insecure TLS switch. Responses, headers and raw status codes are withheld. |
| **New processes / command-line programs** | **Yes — `secure_new_process`** | Human-registered executable/arguments/cwd, environment and/or stdin slots; native per-use consent or a human-created exact-profile Always allow grant. MagicRun owns bounded batch execution and cleanup. Executable bytes are checked against the registered digest. No credential arguments, interactive terminal or raw stdout/stderr/exit-code return. |
| **Already-running processes / interactive terminals** | **Not implemented** | Requires a cooperating input, IPC or credential-provider integration. There is no generic “inject into this PID” or live environment-rewrite capability. |
| **Stateful services / long-lived API clients** | **Not implemented** | Credential refresh, connection pools and existing sessions need an explicit service/provider adapter. Synthetic service-rotation fixtures are preparation, not a working product integration. |
| **Native application password fields / non-Chromium browsers** | **Not implemented** | No accessibility, OS-level secure typing, Firefox or Safari adapter is provided. |
| **Other tools' DOM reads, screenshots, cookies and session output** | **Not filtered** | Browser delivery does not prevent a separate tool from observing secrets afterward. Password masking is not an observation filter. |

Browser fills require a supported writable input and a current document-bound
target. Saved fills require explicit field/origin permission plus per-use or
remembered exact-use consent. [One-time input](jit-credentials.md) uses a native
Use once decision for the captured origins and fields without enrollment.
Headless Chrome still needs the daemon's interactive human-approval host.
A new browser follows the same rules once your tool launches it; MagicVault
does not launch or take ownership of browsers.
See [browser conditions and supported controls](browser-usage.md).


## Interfaces for users and builders

MCP is the recommended local agent interface; [CLI recipes](cli-usage.md) cover
shell-based agents and scripts. Both use the same paired daemon and native per-use or explicitly remembered
consent. The [Node/TypeScript SDK](typescript.md) ships in local npm candidates.
No dedicated Python SDK or remote hosted MCP endpoint is shipped.

Application builders can embed trusted Rust custody/effect crates, implement the
authenticated local protocol, or add the native bridge to an existing Chromium
extension. Libraries and custom extensions must preserve authorization and
value-free agent outputs; they handle plaintext inside their trusted boundary.
See [builder contracts](integrations.md), [architecture](architecture.md) and
[qualification evidence](testing.md). Unimplemented rows are possible future
integrations, not promised releases or dates.
