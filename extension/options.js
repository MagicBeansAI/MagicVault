"use strict";
document.getElementById("extension-id").textContent = chrome.runtime.id;

function sitePattern() {
  const input = document.getElementById("origin").value;
  const url = new URL(input);
  if (url.username || url.password || url.search || url.hash || url.pathname !== "/" ||
      !(url.protocol === "https:" || (url.protocol === "http:" && ["localhost", "127.0.0.1"].includes(url.hostname)))) throw new Error("invalid_origin");
  return `${url.protocol}//${url.hostname}/*`;
}
async function permission(remove) {
  const output = document.getElementById("permission-status");
  try {
    const origins = [sitePattern()];
    const allowed = remove ? await chrome.permissions.remove({origins}) : await chrome.permissions.request({origins});
    output.textContent = allowed ? (remove ? "Site permission removed." : "Site permission granted. Configure matching exact origins in MagicVault.") : "Permission not changed.";
  } catch (_) { output.textContent = "Enter an exact HTTPS origin, or a supported loopback development origin."; }
}
async function control(action) {
  const output = document.getElementById("status");
  try {
    const state = await chrome.runtime.sendMessage({action});
    output.textContent = state.connected ? `Connected. Browser handle: ${state.browser_handle}` : state.connecting ? "Waiting for daemon connection and native human consent. Refresh status after responding." : "Disconnected. Check the documented daemon and native-host setup, then reconnect.";
  } catch (_) { output.textContent = "Connection unavailable. Check the documented setup."; }
}
document.getElementById("allow").addEventListener("click", () => permission(false));
document.getElementById("remove").addEventListener("click", () => permission(true));
document.getElementById("connect").addEventListener("click", () => control("connect"));
document.getElementById("disconnect").addEventListener("click", () => control("disconnect"));
document.getElementById("refresh").addEventListener("click", () => control("status"));
control("status");
