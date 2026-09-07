/* global magicvaultFill */
"use strict";
// Packaging copies the fixed function from magicvault-effect/src/fill.js.
// No remote code, page message receiver, generic evaluate, or enrolled-secret storage.
// Trusted local storage holds site settings and a profile-pairing capability.
importScripts("fill.js");

const HOST = "ai.magicbeans.magicvault";
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
// Chrome document IDs are opaque tokens, commonly 32 uppercase hex digits,
// not our daemon's hyphenated UUID handles. Preserve the exact browser value.
const DOCUMENT_ID = /^(?:[0-9a-f]{32}|[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$/i;
const validDocumentId = value => typeof value === "string" &&
  (value.length === 32 || value.length === 36) && DOCUMENT_ID.test(value);
let channel = null;
let generation = 0;
let browserHandle = null;
let busy = false;
let accessRevision = 0;
let policyWritePending = false;
const POLICY_KEY = "sitePolicy";
// A failed storage restriction must deny operations, not crash worker registration.
const storageReady = chrome.storage.local.setAccessLevel({accessLevel: "TRUSTED_CONTEXTS"})
  .then(() => true, () => false);
chrome.storage.onChanged.addListener((changes, area) => {
  if (area === "local" && Object.hasOwn(changes, POLICY_KEY)) accessRevision++;
});
chrome.permissions.onAdded.addListener(() => { accessRevision++; });
chrome.permissions.onRemoved.addListener(() => { accessRevision++; });

function originOf(value) {
  const url = new URL(value);
  if (url.username || url.password || !(url.protocol === "https:" ||
      (url.protocol === "http:" && ["localhost", "127.0.0.1"].includes(url.hostname)))) throw new Error("unsupported_target");
  if (url.origin.length > 256) throw new Error("unsupported_target");
  return url.origin;
}

function permissionPattern(origin) {
  const url = new URL(origin);
  return `${url.protocol}//${url.hostname}/*`;
}

function siteKey(value) {
  if (typeof value !== "string" || value.length > 256) throw new Error("invalid_site");
  const url = new URL(value);
  originOf(value);
  if (url.pathname !== "/" || url.search || url.hash || url.hostname.includes("*")) throw new Error("invalid_site");
  // Block the exact hostname on every port, including its DNS trailing-dot alias.
  return `${url.protocol}//${url.hostname.replace(/\.$/, "")}`;
}

async function readSitePolicy() {
  if (!await storageReady) throw new Error("policy_unavailable");
  const stored = (await chrome.storage.local.get(POLICY_KEY))[POLICY_KEY];
  // Existing installations retain their Chrome grants and start with no blocks.
  if (stored === undefined) return {version: 1, blockedSites: []};
  if (!stored || stored.version !== 1 || !Array.isArray(stored.blockedSites) ||
      stored.blockedSites.length > 256 ||
      stored.blockedSites.some(site => siteKey(site) !== site) ||
      new Set(stored.blockedSites).size !== stored.blockedSites.length) throw new Error("policy_unavailable");
  return stored;
}

async function changeSiteBlock(site, blocked) {
  // One writer across setup tabs; do not lose concurrent read/modify/write updates.
  if (policyWritePending || typeof blocked !== "boolean") throw new Error("policy_unavailable");
  const key = siteKey(site);
  policyWritePending = true;
  accessRevision++;
  try {
    const policy = await readSitePolicy();
    const sites = new Set(policy.blockedSites);
    if (blocked) sites.add(key); else sites.delete(key);
    if (sites.size > 256) throw new Error("policy_unavailable");
    const updated = {version: 1, blockedSites: [...sites].sort()};
    await chrome.storage.local.set({[POLICY_KEY]: updated});
    return updated;
  } finally {
    policyWritePending = false;
    accessRevision++;
  }
}

function accessUnchanged(revision) {
  return !policyWritePending && accessRevision === revision;
}

async function permitted(origin, policy) {
  return !policy.blockedSites.includes(siteKey(origin)) &&
    await chrome.permissions.contains({origins: [permissionPattern(origin)]});
}

function active(port, epoch) {
  if (channel !== port || generation !== epoch || !browserHandle) throw new Error("stale_session");
}

const CONNECTION_KEY = "connection";
const RETRY_ALARM = "magicvault-reconnect";
const WATCHDOG_ALARM = "magicvault-handshake";
const NATIVE_VERSION = 2;
const TRANSIENT = ["busy", "transport_unavailable", "transport_uncertain"];
const REASONS = ["paused", "denied", "unauthorized", "conflict", "capacity", "unsupported_version",
  "invalid_request", "unavailable", "expired", "cancelled", "persistence_uncertain", "storage_unavailable",
  ...TRANSIENT];
let connection = null;
let pausedNow = false;
let connectionError = null;
let connectionWork = Promise.resolve();

function closeChannel() {
  const old = channel;
  channel = null;
  browserHandle = null;
  generation++;
  busy = false;
  if (old) { try { old.disconnect(); } catch (_) {} }
}

function connectionStatus() {
  return {connected: Boolean(browserHandle), connecting: Boolean(channel) && !browserHandle,
    browser_handle: browserHandle, profile_id: connection?.profile_id || null,
    paused: pausedNow || connection?.mode === "paused", reason: connectionError || connection?.reason || null,
    retry_at: connection?.mode === "enabled" ? connection.retryAt : 0};
}

function transition(work) {
  // A single writer covers startup, alarms, native events and multiple setup tabs.
  connectionWork = connectionWork.then(work).catch(async () => {
    pausedNow = true;
    connectionError = "storage_unavailable";
    closeChannel();
    await Promise.allSettled([chrome.alarms.clear(RETRY_ALARM), chrome.alarms.clear(WATCHDOG_ALARM)]);
  });
  return connectionWork;
}

async function saveConnection() {
  if (!await storageReady || !connection) throw new Error("storage_unavailable");
  await chrome.storage.local.set({[CONNECTION_KEY]: connection});
}

async function stopConnection(reason) {
  closeChannel();
  connection.mode = "paused";
  connection.reason = reason;
  connection.retryAt = 0;
  await saveConnection();
  await chrome.alarms.clear(RETRY_ALARM);
  await chrome.alarms.clear(WATCHDOG_ALARM);
}

async function failedConnection(code) {
  if (pausedNow || connection?.mode === "paused") return;
  closeChannel();
  await chrome.alarms.clear(WATCHDOG_ALARM);
  if (!TRANSIENT.includes(code)) return stopConnection(REASONS.includes(code) ? code : "invalid_request");
  connection.failures = Math.min(connection.failures + 1, 5);
  // MV3 alarms survive worker suspension; Chrome 120's minimum is 30 seconds.
  // Keep the deadline durable so unrelated worker wakeups cannot bypass backoff.
  const delay = Math.min(30 * 2 ** (connection.failures - 1), 300);
  connection.retryAt = Date.now() + (delay + Math.floor(Math.random() * 6)) * 1000;
  connection.reason = code;
  await saveConnection();
  if (!pausedNow && connection.mode === "enabled") {
    await chrome.alarms.create(RETRY_ALARM, {when: connection.retryAt});
  }
}

function failChannel(code, port, epoch) {
  if (channel !== port || generation !== epoch) return;
  closeChannel();
  const closedEpoch = generation;
  void transition(async () => {
    if (generation === closedEpoch) await failedConnection(code);
  });
}

async function connect(manual = false) {
  if (channel || pausedNow || connection?.mode !== "enabled") return;
  if (!manual && connection.retryAt > Date.now()) {
    await chrome.alarms.create(RETRY_ALARM, {when: connection.retryAt});
    return;
  }
  const epoch = ++generation;
  let port;
  try { port = chrome.runtime.connectNative(HOST); }
  catch (_) { return failedConnection("transport_unavailable"); }
  channel = port;
  port.onDisconnect.addListener(() => {
    void chrome.runtime.lastError; // Do not reflect native/browser diagnostics.
    if (channel !== port || generation !== epoch) return;
    failChannel("transport_unavailable", port, epoch);
  });
  port.onMessage.addListener(message => {
    if (channel !== port || epoch !== generation) return;
    if (message?.kind === "error") {
      const code = message.version === NATIVE_VERSION ? message.code : "unsupported_version";
      failChannel(code, port, epoch);
      return;
    }
    if (!browserHandle) {
      if (message?.kind !== "ready" || message.version !== NATIVE_VERSION || !UUID.test(message.browser_handle)) {
        failChannel("unsupported_version", port, epoch);
        return;
      }
      browserHandle = message.browser_handle;
      void transition(async () => {
        if (channel !== port || generation !== epoch) return;
        connection.failures = 0; connection.retryAt = 0; connection.reason = null;
        await saveConnection();
        await chrome.alarms.clear(RETRY_ALARM);
        await chrome.alarms.clear(WATCHDOG_ALARM);
      });
      return;
    }
    if (busy || !UUID.test(message?.request_id) || !message.request ||
        !["targets", "filtered_targets", "fill"].includes(message.request.method)) {
      failChannel("invalid_request", port, epoch); return;
    }
    busy = true;
    handle(message.request, port, epoch).then(result => {
      if (channel === port && generation === epoch) port.postMessage({request_id: message.request_id, result});
    }).catch(() => {
      // Reconnect transport, never replay an operation with uncertain effects.
      if (channel === port && generation === epoch) {
        failChannel("transport_uncertain", port, epoch);
      }
    }).finally(() => { if (generation === epoch) busy = false; });
  });
  try {
    // Profile capability never goes to the options page, agent or website.
    port.postMessage({version: NATIVE_VERSION, profile_id: connection.profile_id,
      capability: connection.capability, manual});
    await chrome.alarms.create(WATCHDOG_ALARM, {delayInMinutes: 4});
  } catch (_) {
    if (channel === port && generation === epoch) await failedConnection("transport_unavailable");
  }
}

async function initializeConnection() {
  if (!await storageReady) throw new Error("storage_unavailable");
  const stored = (await chrome.storage.local.get(CONNECTION_KEY))[CONNECTION_KEY];
  if (stored === undefined) {
    connection = {version: 1, profile_id: crypto.randomUUID(),
      capability: Array.from(crypto.getRandomValues(new Uint8Array(32)), x => x.toString(16).padStart(2, "0")).join(""),
      mode: "enabled", failures: 0, retryAt: 0, reason: null};
    await saveConnection();
  } else {
    if (!stored || stored.version !== 1 || !UUID.test(stored.profile_id) ||
        !/^[0-9a-f]{64}$/.test(stored.capability) || !["enabled", "paused"].includes(stored.mode) ||
        !Number.isInteger(stored.failures) || stored.failures < 0 || stored.failures > 5 ||
        !Number.isSafeInteger(stored.retryAt) || stored.retryAt < 0 ||
        (stored.reason !== null && !REASONS.includes(stored.reason))) throw new Error("storage_unavailable");
    connection = stored;
    // A clock adjustment must not suspend recovery indefinitely.
    connection.retryAt = Math.min(connection.retryAt, Date.now() + 305000);
  }
  await chrome.alarms.clear(WATCHDOG_ALARM);
  if (connection.mode === "paused" || pausedNow) {
    await chrome.alarms.clear(RETRY_ALARM);
    return;
  }
  await connect();
}

async function targets(port, epoch, filter) {
  if (!filter || typeof filter !== "object" || Array.isArray(filter) ||
      Object.keys(filter).some(key => !["top_origin", "tab_id"].includes(key)) ||
      (Object.hasOwn(filter, "top_origin") && typeof filter.top_origin !== "string") ||
      (Object.hasOwn(filter, "tab_id") && (typeof filter.tab_id !== "string" ||
        !/^(?:0|[1-9][0-9]{0,9})$/.test(filter.tab_id)))) return {kind: "error", data: "invalid_request"};
  if (Object.hasOwn(filter, "top_origin")) {
    try { siteKey(filter.top_origin); if (originOf(filter.top_origin) !== filter.top_origin) throw new Error(); }
    catch (_) { return {kind: "error", data: "invalid_request"}; }
  }
  const revision = accessRevision;
  let policy;
  let origins;
  try {
    policy = await readSitePolicy();
    ({origins = []} = await chrome.permissions.getAll());
  }
  catch (_) { return {kind: "error", data: "permission_denied"}; }
  active(port, epoch);
  if (!accessUnchanged(revision)) return {kind: "error", data: "permission_denied"};
  if (!origins.length) return {kind: "targets", data: []};
  // A caller's narrowing is never a grant. Verify it against current Chrome
  // authority before querying even a requested site's tab metadata.
  if (filter.top_origin) {
    const allowed = await permitted(filter.top_origin, policy);
    active(port, epoch);
    if (!accessUnchanged(revision)) return {kind: "error", data: "permission_denied"};
    if (!allowed) return {kind: "targets", data: []};
    origins = [permissionPattern(filter.top_origin)];
  }
  // Host grants allow URL queries without the broad "tabs" permission. Chrome
  // may omit a URL (e.g. after revocation); never fall back to inspecting it.
  // Discarded tabs have no loaded document. Discovery must not wake them.
  const tabs = await chrome.tabs.query({url: origins, discarded: false});
  active(port, epoch);
  // Cache only for this revision/operation, including repeated cross-origin
  // frames. Fills still perform their own fresh, uncached permission checks.
  const permissions = new Map();
  const permittedHere = async origin => {
    if (policy.blockedSites.includes(siteKey(origin))) return false;
    const pattern = permissionPattern(origin);
    if (!permissions.has(pattern)) {
      permissions.set(pattern, await chrome.permissions.contains({origins: [pattern]}));
    }
    return permissions.get(pattern);
  };
  const candidates = [];
  for (const tab of tabs) {
    active(port, epoch);
    if (!accessUnchanged(revision)) return {kind: "error", data: "permission_denied"};
    if (tab.discarded || !Number.isInteger(tab.id) || tab.id < 0 ||
        typeof tab.url !== "string") continue;
    if (filter.tab_id && String(tab.id) !== filter.tab_id) continue;
    let origin;
    try { origin = originOf(tab.url); } catch (_) { continue; }
    // Chrome match patterns ignore ports; the actual origin must still match.
    if (filter.top_origin && origin !== filter.top_origin) continue;
    if (!await permittedHere(origin)) continue;
    active(port, epoch);
    if (!accessUnchanged(revision)) return {kind: "error", data: "permission_denied"};
    candidates.push(tab.id);
    // Bound expensive frame inspection, not the profile's unrelated tab count.
    // Never silently truncate a discovery response to fit the wire budget.
    if (candidates.length > 128) return {kind: "error", data: "capacity"};
  }
  const result = [];
  for (const tabId of candidates) {
    active(port, epoch);
    if (!accessUnchanged(revision)) return {kind: "error", data: "permission_denied"};
    let frames;
    try { frames = await chrome.webNavigation.getAllFrames({tabId}); }
    catch (_) { continue; }
    active(port, epoch);
    if (!accessUnchanged(revision)) return {kind: "error", data: "permission_denied"};
    if (!frames || frames.length > 128) continue;
    const top = frames.find(frame => frame.frameId === 0 && frame.documentLifecycle === "active");
    if (!top || top.errorOccurred || !validDocumentId(top.documentId)) continue;
    let topOrigin;
    try { topOrigin = originOf(top.url); } catch (_) { continue; }
    if (filter.top_origin && topOrigin !== filter.top_origin) continue;
    if (!await permittedHere(topOrigin)) continue;
    for (const frame of frames) {
      active(port, epoch);
      if (!accessUnchanged(revision)) return {kind: "error", data: "permission_denied"};
      if (frame.errorOccurred || frame.documentLifecycle !== "active" || !validDocumentId(frame.documentId)) continue;
      let origin;
      try { origin = originOf(frame.url); } catch (_) { continue; }
      if (!await permittedHere(origin)) continue;
      result.push({tab: String(tabId), frame: String(frame.frameId), document: frame.documentId,
        top_document: top.documentId, origin, top_origin: topOrigin, is_main_frame: frame.frameId === 0});
      if (result.length > 128) return {kind: "error", data: "capacity"};
    }
  }
  active(port, epoch);
  if (!accessUnchanged(revision)) return {kind: "error", data: "permission_denied"};
  return {kind: "targets", data: result};
}

async function fill(params, port, epoch) {
  const fields = params?.fields;
  const target = params?.target;
  if (!Array.isArray(fields) || fields.length === 0 || fields.length > 8 || !target) throw new Error("invalid_request");
  const failed = error => ({kind: "filled", data: {fields: fields.map(() => "not_filled"), error}});
  const uncertain = () => ({kind: "filled", data: {fields: fields.map(() => "uncertain"), error: "transport_uncertain"}});
  try {
    if (!/^[0-9]{1,10}$/.test(target.tab) || !/^[0-9]{1,10}$/.test(target.frame) ||
        !validDocumentId(target.document) || !validDocumentId(target.top_document) ||
        originOf(target.origin) !== target.origin || originOf(target.top_origin) !== target.top_origin ||
        fields.some(field => typeof field.css !== "string" || field.css.length > 512 ||
          typeof field.value !== "string" || field.value.length === 0 || field.value.length > 4096)) return failed("invalid_request");
    const revision = accessRevision;
    let policy;
    try { policy = await readSitePolicy(); } catch (_) { return failed("permission_denied"); }
    if (!accessUnchanged(revision) || !await permitted(target.origin, policy) ||
        !await permitted(target.top_origin, policy)) return failed("permission_denied");
    active(port, epoch);
    const tabId = Number(target.tab);
    const frameId = Number(target.frame);
    const frame = await chrome.webNavigation.getFrame({tabId, frameId, documentId: target.document});
    const top = await chrome.webNavigation.getFrame({tabId, frameId: 0, documentId: target.top_document});
    active(port, epoch);
    if (!frame || !top || frame.errorOccurred || top.errorOccurred ||
        frame.documentLifecycle !== "active" || top.documentLifecycle !== "active" ||
        frame.documentId !== target.document || top.documentId !== target.top_document ||
        originOf(frame.url) !== target.origin || originOf(top.url) !== target.top_origin ||
        target.is_main_frame !== (frameId === 0)) return failed("stale_target");
    // Re-read after asynchronous document checks. Previously discovered handles
    // never bypass a new block or Chrome permission revocation.
    try { policy = await readSitePolicy(); } catch (_) { return failed("permission_denied"); }
    if (!await permitted(target.origin, policy) || !await permitted(target.top_origin, policy) ||
        !accessUnchanged(revision)) return failed("permission_denied");
    active(port, epoch);
    // documentIds, not frameIds: navigation must fail rather than selecting a
    // replacement document between preflight and execution. Isolated world
    // avoids page-overridden JS primitives; the resulting DOM is still shared.
    let results;
    try {
      results = await chrome.scripting.executeScript({target: {tabId, documentIds: [target.document]},
        world: "ISOLATED", func: magicvaultFill, args: [target.origin, fields]});
    } catch (_) { return uncertain(); }
    active(port, epoch);
    if (results.length !== 1 || results[0].documentId !== target.document || results[0].frameId !== frameId) return uncertain();
    const outcome = results[0].result;
    const errors = [null, "invalid_request", "stale_target", "unsupported_target", "ambiguous_target", "unavailable"];
    if (!outcome || !Array.isArray(outcome.fields) || outcome.fields.length !== fields.length ||
        outcome.fields.some(state => !["filled", "not_filled", "uncertain"].includes(state)) || !errors.includes(outcome.error)) return uncertain();
    return {kind: "filled", data: {fields: outcome.fields, error: outcome.error}};
  } catch (_) {
    return failed("stale_target");
  } finally {
    for (const field of fields) if (field && typeof field === "object") field.value = "";
  }
}

async function handle(request, port, epoch) {
  active(port, epoch);
  if (request.method === "targets") return targets(port, epoch, {});
  if (request.method === "filtered_targets") return targets(port, epoch, request.params);
  return fill(request.params, port, epoch);
}

chrome.runtime.onMessage.addListener((message, sender, reply) => {
  // Only our own extension setup page; no content-script or website command
  // path. The setup page never handles credential material.
  if (sender.id !== chrome.runtime.id || sender.url !== chrome.runtime.getURL("options.html")) return false;
  if (message?.action === "site-policy" || message?.action === "site-block") {
    const operation = message.action === "site-policy" ? readSitePolicy() : changeSiteBlock(message.site, message.blocked);
    operation.then(policy => reply({ok: true, blockedSites: policy.blockedSites}),
      () => reply({ok: false}));
    return true;
  }
  if (message?.action === "disconnect") {
    pausedNow = true;
    closeChannel(); // Invalidate in-flight preflight before any asynchronous write.
    transition(() => stopConnection("paused")).then(() => reply(connectionStatus()));
  } else if (message?.action === "connect") {
    pausedNow = false;
    transition(async () => {
      if (!connection || connectionError) throw new Error("storage_unavailable");
      connection.mode = "enabled"; connection.retryAt = 0; connection.failures = 0; connection.reason = null;
      await saveConnection(); await connect(true);
    }).then(() => reply(connectionStatus()));
  } else if (message?.action === "status") {
    connectionWork.then(() => reply(connectionStatus()));
  } else return false;
  return true;
});
chrome.action.onClicked.addListener(() => chrome.runtime.openOptionsPage());
chrome.runtime.onInstalled.addListener(() => chrome.runtime.openOptionsPage());
chrome.runtime.onStartup.addListener(() => { void transition(() => connect()); });
chrome.alarms.onAlarm.addListener(alarm => {
  if (alarm.name === RETRY_ALARM) void transition(() => connect());
  else if (alarm.name === WATCHDOG_ALARM) void transition(async () => {
    if (channel && !browserHandle) await failedConnection("expired");
  });
});
void transition(initializeConnection);
