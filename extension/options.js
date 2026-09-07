"use strict";
const element = id => document.getElementById(id);
element("extension-id").textContent = chrome.runtime.id;
let changing = false;
let refreshRevision = 0;
let blockRevision = 0;
let allHttpsEnabled = false;
const accessButtons = ["allow-all", "selected-only", "allow", "remove", "block", "unblock"];

function updateAccessButtons() {
  for (const id of accessButtons) element(id).disabled = changing;
  element("allow-all").disabled = changing || allHttpsEnabled;
  element("allow-all").textContent = allHttpsEnabled ? "All HTTPS websites enabled" : "Allow all HTTPS websites";
}

function siteOrigin() {
  const input = element("origin").value;
  if (input.length > 256) throw new Error("invalid_origin");
  const url = new URL(input);
  if (url.username || url.password || url.search || url.hash || url.pathname !== "/" || url.hostname.includes("*") ||
      !(url.protocol === "https:" || (url.protocol === "http:" && ["localhost", "127.0.0.1"].includes(url.hostname)))) throw new Error("invalid_origin");
  return url.origin;
}

async function refreshBrowserAccess() {
  const revision = ++refreshRevision;
  try {
    const [all, grants] = await Promise.all([
      chrome.permissions.contains({origins: ["https://*/*"]}),
      chrome.permissions.getAll()
    ]);
    if (revision !== refreshRevision) return;
    allHttpsEnabled = all;
    const origins = grants.origins || [];
    element("access-mode").textContent = all ? "All HTTPS websites enabled" : origins.length ? "Selected websites only" : "No websites allowed yet";
    element("access-summary").dataset.state = all ? "all" : origins.length ? "selected" : "none";
    element("granted-sites").textContent = origins.join(", ") || "No website permissions granted.";
    updateAccessButtons();
  } catch (_) {
    if (revision !== refreshRevision) return;
    allHttpsEnabled = false;
    element("access-mode").textContent = "Browser access could not be verified";
    element("access-summary").dataset.state = "unknown";
    element("granted-sites").textContent = "Browser permissions unavailable. Refresh to check again.";
    updateAccessButtons();
  }
}

async function refreshBlockedSites() {
  const revision = ++blockRevision;
  try {
    const policy = await chrome.runtime.sendMessage({action: "site-policy"});
    if (revision !== blockRevision) return;
    element("blocked-sites").replaceChildren();
    if (!policy?.ok) throw new Error("policy_unavailable");
    for (const site of policy.blockedSites) {
      const option = document.createElement("option");
      option.value = site;
      option.textContent = site;
      element("blocked-sites").append(option);
    }
    element("block-status").textContent = policy.blockedSites.length ? `${policy.blockedSites.length} site(s) disabled in MagicVault.` : "No sites disabled in MagicVault.";
  } catch (_) {
    if (revision === blockRevision) {
      element("blocked-sites").replaceChildren();
      element("block-status").textContent = "Blocklist unavailable. MagicVault refuses fills if its site policy cannot be read.";
    }
  }
}

async function refreshAccess() {
  // Chrome's grant is independent of native connection and blocklist availability.
  // A slow/failed worker must not hide an already confirmed browser permission.
  await Promise.all([refreshBrowserAccess(), refreshBlockedSites()]);
}

async function changeAccess(action) {
  if (changing) return;
  changing = true;
  updateAccessButtons();
  const output = element("permission-status");
  try {
    output.textContent = "Applying change. If Chrome asks for permission, approve or dismiss its prompt.";
    // Call the action before yielding: permission requests require a user gesture.
    output.textContent = await action();
  } catch (_) {
    output.textContent = "Change failed. Use an exact HTTPS origin or supported local HTTP origin; check permissions and refresh before retrying.";
  } finally {
    await refreshAccess();
    changing = false;
    updateAccessButtons();
  }
}

element("allow-all").addEventListener("click", () => changeAccess(async () => {
  const allowed = await chrome.permissions.request({origins: ["https://*/*"]});
  return allowed ? "All HTTPS access granted. Disabled sites and credential approvals still apply. Local HTTP was not enabled." : "All HTTPS access was not granted. Existing settings retained.";
}));
element("selected-only").addEventListener("click", () => changeAccess(async () => {
  // Reset HTTPS grants explicitly; do not claim to preserve narrower grants when
  // Chrome removes an overlapping wildcard. Local HTTP grants are independent.
  const origins = (await chrome.permissions.getAll()).origins?.filter(origin => origin.startsWith("https://")) || [];
  if (origins.length && !await chrome.permissions.remove({origins})) throw new Error("permission_denied");
  if (await chrome.permissions.contains({origins: ["https://*/*"]})) throw new Error("permission_denied");
  return "HTTPS grants cleared. Allow individual sites below. Local HTTP grants and disabled sites are unchanged.";
}));
element("allow").addEventListener("click", () => changeAccess(async () => {
  const url = new URL(siteOrigin());
  const allowed = await chrome.permissions.request({origins: [`${url.protocol}//${url.hostname}/*`]});
  return allowed ? "Site permission granted. A disabled site stays blocked until re-enabled; exact credential origins still need MagicVault approval." : "Site permission was not granted.";
}));
element("remove").addEventListener("click", () => changeAccess(async () => {
  const url = new URL(siteOrigin());
  if (url.protocol === "https:" && await chrome.permissions.contains({origins: ["https://*/*"]})) {
    return "All HTTPS access is enabled. Use Disable site, or clear HTTPS access and allow only selected sites.";
  }
  const origins = [`${url.protocol}//${url.hostname}/*`];
  if (!await chrome.permissions.remove({origins}) || await chrome.permissions.contains({origins})) throw new Error("permission_denied");
  return "Site permission removed. Other grants and disabled sites are unchanged.";
}));
async function setBlock(site, blocked) {
  const response = await chrome.runtime.sendMessage({action: "site-block", site, blocked});
  if (!response?.ok) throw new Error("policy_unavailable");
  return blocked ? "Site disabled for MagicVault discovery and fills. Chrome's underlying permission is unchanged; already dispatched fills cannot be recalled." : "Site re-enabled in MagicVault. Browser permission and credential approval are still required.";
}
element("block").addEventListener("click", () => changeAccess(() => setBlock(siteOrigin(), true)));
element("unblock").addEventListener("click", () => changeAccess(() => setBlock(element("blocked-sites").value, false)));
element("refresh-access").addEventListener("click", refreshAccess);
chrome.permissions.onAdded.addListener(refreshAccess);
chrome.permissions.onRemoved.addListener(refreshAccess);
chrome.storage.onChanged.addListener((changes, area) => {
  if (area === "local" && Object.hasOwn(changes, "sitePolicy")) void refreshAccess();
  if (area === "local" && Object.hasOwn(changes, "connection")) void control("status");
});
window.addEventListener("focus", () => { void refreshAccess(); });
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") void refreshAccess();
});
async function control(action) {
  const output = document.getElementById("status");
  try {
    const state = await chrome.runtime.sendMessage({action});
    element("profile-id").textContent = state.profile_id || "Not available";
    const reasons = {paused: "Paused by you.", denied: "Browser authorization was denied or revoked.",
      unauthorized: "Pairing is missing or revoked. Check MagicVault setup.", conflict: "Another connection is using this profile identity. Close the copied profile before retrying.",
      capacity: "MagicVault's browser or pairing limit was reached.", unsupported_version: "Update MagicVault and reload the matching extension.",
      storage_unavailable: "Local connection settings could not be saved or read. Reload after fixing storage; no automatic retry.",
      expired: "The connection approval timed out.", cancelled: "Connection approval was cancelled."};
    output.textContent = state.connected ? `Connected. Browser handle: ${state.browser_handle}` : state.connecting ? "Connecting to MagicVault. Approve the native dialog if this is the first connection for this profile." :
      state.paused ? `${reasons[state.reason] || "Connection stopped for safety. Check MagicVault setup."} Choose Retry / resume when ready.` :
      state.retry_at ? `MagicVault is unavailable or busy. Automatic retry after ${new Date(state.retry_at).toLocaleTimeString()} (Chrome may delay it). Run magicvault setup if the native bridge is not installed.` : "Waiting for automatic connection.";
    element("connect").disabled = state.connected || state.connecting;
  } catch (_) { output.textContent = "Connection unavailable. Check the documented setup."; }
}
document.getElementById("connect").addEventListener("click", () => control("connect"));
document.getElementById("disconnect").addEventListener("click", () => control("disconnect"));
document.getElementById("refresh").addEventListener("click", () => control("status"));
void refreshAccess();
void control("status");
