# Private prompts

MagicVault's daemon writes the prompt text. An agent provides non-secret request
metadata: its paired client identity, field names and selectors. The daemon adds
the browser-discovered destination, storage behavior and approval scope. No model
summarizes an authorization request or chooses its buttons.

The desktop window uses a shared Rust/egui implementation on macOS, Windows and
Linux. It replaces the macOS-only AppleScript dialogs, whose dark appearance
came from the system dialog renderer. A light surface, readable spacing, masked
input and expandable **Request details** provide the same presentation across
platforms. A logged-in graphical session is required; see [platforms](platforms.md).

## What you review

- **Summary:** the action, requesting client, browser, destination and whether
  input is saved or used once. Cross-origin frame destinations remain explicit.
- **Request details:** complete field-to-selector mappings and technical IDs.
  These are untrusted request data, displayed literally, never executed or
  silently truncated. Other administrative requests retain their full context.
- **Decision:** Cancel is initially selected for approvals. Typing a value does
  not approve a fill. One-time input ends with a separate **Use once** decision;
  saved use can offer an unchecked **Always allow this exact use** choice.

An input window shows the current field rather than repeating every requested
selector. The final fill prompt retains the full mapping. No values appear in
the summary or details. Names and labels are visible metadata: never put secrets
in them. Saved enrollment collects all fields before writing a record.

## Where answers go

The daemon starts its adjacent `magicvault-prompt` executable directly. It sends
bounded metadata through stdin and receives a closed decision or masked-input
answer through a private stdout pipe. Values are not placed in command arguments,
environment variables, request files, MCP replies or Node SDK results. The helper
has no HTTP listener, browser page, analytics or persisted application state.
It cannot enroll or deliver credentials itself.

Closing the window, Escape, cancellation, timeout, malformed replies or loss of
the parent connection refuse the operation. The daemon kills/reaps cancelled
helpers before releasing its human-operation slot. Missing/broken UI fails closed;
there is no chat or terminal password fallback.

Owned secret buffers are zeroized on drop. GUI text editing, OS memory and the
destination browser are outside a complete-memory-erasure guarantee. The website
receives filled values, and another browser tool may read them afterward.

## Embedded applications

`HumanInteraction` remains the trusted host seam for another UI. Magician uses its
own private HITL integration, independent of this standalone window. Updating
MagicVault does not restyle Magician or route credentials through ordinary agent
messages. See [integrations](integrations.md).
