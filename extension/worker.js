/* global magicvaultFill */
"use strict";
// Packaging copies the fixed function from magicvault-effect/src/fill.js.
// No remote code, page message receiver, generic evaluate, or secret storage.
importScripts("fill.js");

const HOST = "ai.magicbeans.magicvault";
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
let channel = null;
let generation = 0;
let browserHandle = null;
let busy = false;

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

async function permitted(origin) {
  return chrome.permissions.contains({origins: [permissionPattern(origin)]});
}

function active(port, epoch) {
  if (channel !== port || generation !== epoch || !browserHandle) throw new Error("stale_session");
}

function disconnect() {
  const old = channel;
  channel = null;
  browserHandle = null;
  generation++;
  busy = false;
  if (old) { try { old.disconnect(); } catch (_) {} }
}

function connect() {
  if (channel) return;
  generation++;
  const epoch = generation;
  const port = chrome.runtime.connectNative(HOST);
  channel = port;
  port.onDisconnect.addListener(() => {
    // Read and discard the browser diagnostic; never log native host details.
    void chrome.runtime.lastError;
    if (channel === port) disconnect();
  });
  port.onMessage.addListener(message => {
    if (channel !== port || epoch !== generation) return;
    if (!browserHandle) {
      if (message?.kind !== "ready" || message.version !== 1 || !UUID.test(message.browser_handle)) { disconnect(); return; }
      browserHandle = message.browser_handle;
      return;
    }
    if (busy || !UUID.test(message?.request_id) || !message.request ||
        !["targets", "fill"].includes(message.request.method)) { disconnect(); return; }
    busy = true;
    handle(message.request, port, epoch).then(result => {
      if (channel === port && generation === epoch) port.postMessage({request_id: message.request_id, result});
    }).catch(() => {
      // Unexpected failures close the channel. Daemon records uncertain effect,
      // rather than reflecting an exception or optimistically reporting failure.
      if (channel === port && generation === epoch) disconnect();
    }).finally(() => { if (generation === epoch) busy = false; });
  });
}

async function targets(port, epoch) {
  const tabs = await chrome.tabs.query({});
  active(port, epoch);
  if (tabs.length > 64) return {kind: "error", data: "capacity"};
  const result = [];
  for (const tab of tabs) {
    let frames;
    try { frames = await chrome.webNavigation.getAllFrames({tabId: tab.id}); }
    catch (_) { continue; }
    active(port, epoch);
    if (!frames || frames.length > 128) continue;
    const top = frames.find(frame => frame.frameId === 0 && frame.documentLifecycle === "active");
    if (!top || !UUID.test(top.documentId)) continue;
    let topOrigin;
    try { topOrigin = originOf(top.url); } catch (_) { continue; }
    if (!await permitted(topOrigin)) continue;
    for (const frame of frames) {
      active(port, epoch);
      if (frame.errorOccurred || frame.documentLifecycle !== "active" || !UUID.test(frame.documentId)) continue;
      let origin;
      try { origin = originOf(frame.url); } catch (_) { continue; }
      if (!await permitted(origin)) continue;
      result.push({tab: String(tab.id), frame: String(frame.frameId), document: frame.documentId,
        top_document: top.documentId, origin, top_origin: topOrigin, is_main_frame: frame.frameId === 0});
      if (result.length > 128) return {kind: "error", data: "capacity"};
    }
  }
  active(port, epoch);
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
        !UUID.test(target.document) || !UUID.test(target.top_document) ||
        originOf(target.origin) !== target.origin || originOf(target.top_origin) !== target.top_origin ||
        fields.some(field => typeof field.css !== "string" || field.css.length > 512 ||
          typeof field.value !== "string" || field.value.length === 0 || field.value.length > 4096)) return failed("invalid_request");
    if (!await permitted(target.origin) || !await permitted(target.top_origin)) return failed("permission_denied");
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
  if (request.method === "targets") return targets(port, epoch);
  return fill(request.params, port, epoch);
}

chrome.runtime.onMessage.addListener((message, sender, reply) => {
  // Only our own extension setup page; no content-script or website command
  // path. The setup page never handles credential material.
  if (sender.id !== chrome.runtime.id || sender.url !== chrome.runtime.getURL("options.html")) return false;
  if (message?.action === "connect") connect();
  else if (message?.action === "disconnect") disconnect();
  else if (message?.action !== "status") return false;
  reply({connected: Boolean(browserHandle), connecting: Boolean(channel) && !browserHandle, browser_handle: browserHandle});
  return false;
});
chrome.action.onClicked.addListener(() => chrome.runtime.openOptionsPage());
chrome.runtime.onInstalled.addListener(() => chrome.runtime.openOptionsPage());
