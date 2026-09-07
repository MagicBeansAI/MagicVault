"use strict";
// Only a fixed synthetic canary is compared. Never render or transmit values.
const FIXTURE_CANARY = "SYNTHETIC-CHROMIUM-FILL-CANARY";
let inputEvents = 0;
let submissions = 0;
function render() {
  document.getElementById("observations").textContent =
    `Input events: ${inputEvents}. Synthetic canary matched: ${document.body.dataset.reactive === "ok"}. Submissions: ${submissions}.`;
}
document.getElementById("password").addEventListener("input", event => {
  inputEvents++;
  document.body.dataset.reactive = event.target.value === FIXTURE_CANARY ? "ok" : "other";
  render();
});
document.getElementById("login").addEventListener("submit", event => {
  event.preventDefault();
  submissions++;
  document.body.dataset.submitted = "yes";
  render();
});
document.getElementById("replace-first").addEventListener("input", () => {
  const previous = document.getElementById("replace-later");
  const replacement = document.createElement("input");
  replacement.id = "replace-later";
  previous.replaceWith(replacement);
});
const shadow = document.getElementById("shadow-host").attachShadow({mode: "open"});
const shadowInput = document.createElement("input");
shadowInput.id = "shadow-input";
shadow.append(shadowInput);
if (location.pathname === "/frames") {
  const cases = document.getElementById("frame-cases");
  cases.hidden = false;
  for (const opaque of [false, true]) {
    const frame = document.createElement("iframe");
    frame.id = opaque ? "opaque-frame" : "same-origin-frame";
    frame.title = opaque ? "Opaque sandbox frame: must refuse" : "Same-origin frame";
    if (opaque) frame.setAttribute("sandbox", "allow-scripts");
    frame.src = "/frame";
    cases.append(frame);
  }
}
if (location.pathname === "/frame") document.getElementById("control-cases").hidden = true;
