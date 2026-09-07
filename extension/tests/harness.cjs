// Synthetic Chrome API boundary; actual worker, options and fixed fill sources.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const {webcrypto} = require("node:crypto");
const ID = "a".repeat(32);
const DOC = "11111111-1111-4111-8111-111111111111";
const OP = "22222222-2222-4222-8222-222222222222";
const FRAME = "33333333-3333-4333-8333-333333333333";
const source = path.resolve(__dirname, "..");
function event() {
  const listeners = [];
  return {listeners, addListener: fn => listeners.push(fn), emit: (...args) => listeners.map(fn => fn(...args))};
}
function fixture({directory, grants = ["https://example.com/*"], stored = {}, accessError = false} = {}) {
  const f = {now: Date.now(), sent: [], hellos: [], ports: [], alarms: new Map(), connectionWrites: [], executions: 0, writes: [], reads: 0, stored: structuredClone(stored),
    grants: new Set(grants), permission: true, requests: [], removals: [], connects: 0,
    frames: [{frameId: 0, documentId: DOC, url: "https://example.com/login?token=SYNTHETIC-URL-CANARY", documentLifecycle: "active"}]};
  const newPort = () => ({onMessage: event(), onDisconnect: event(), postMessage: value => {
    if (value.capability) f.hellos.push(structuredClone(value)); else f.sent.push(value);
  }, disconnect() { f.disconnected = true; }});
  let port = newPort();
  const chrome = {
    runtime: {id: ID, getURL: file => `chrome-extension://${ID}/${file}`, onMessage: event(), onInstalled: event(), onStartup: event(),
      connectNative: () => { f.connects++; port = newPort(); f.ports.push(port); return port; }, openOptionsPage() {}},
    alarms: {onAlarm: event(), create: async (name, info) => { f.alarms.set(name, info); },
      clear: async name => f.alarms.delete(name)},
    action: {onClicked: event()},
    storage: {onChanged: event(), local: {
      setAccessLevel: async options => { assert.equal(options.accessLevel, "TRUSTED_CONTEXTS"); if (accessError) throw new Error("SYNTHETIC-ACCESS-ERROR"); f.restricted = true; },
      get: async key => {
        if (key === "sitePolicy") f.reads++;
        if (f.readError) throw new Error("SYNTHETIC-STORAGE-ERROR");
        if (f.onRead && key === "sitePolicy") await f.onRead();
        return {[key]: structuredClone(f.stored[key])};
      },
      set: async value => {
        if (f.onWrite && value.sitePolicy) await f.onWrite();
        if (f.writeError) throw new Error("SYNTHETIC-STORAGE-ERROR");
        (value.connection ? f.connectionWrites : f.writes).push(structuredClone(value)); Object.assign(f.stored, structuredClone(value));
        chrome.storage.onChanged.emit(Object.fromEntries(Object.entries(value).map(([k, v]) => [k, {newValue: v}])), "local");
      }
    }},
    permissions: {
      onAdded: event(), onRemoved: event(),
      contains: async ({origins}) => f.permission && origins.every(origin => f.grants.has(origin) || (origin.startsWith("https://") && f.grants.has("https://*/*"))),
      getAll: async () => ({origins: [...f.grants]}),
      request: async ({origins}) => {
        assert.equal(f.gesture, true, "permission request must begin synchronously in a click");
        f.requests.push([...origins]);
        if (f.onRequest) await f.onRequest();
        if (f.rejectRequest) return false;
        for (const origin of origins) f.grants.add(origin);
        chrome.permissions.onAdded.emit({origins}); return true;
      },
      remove: async ({origins}) => {
        f.removals.push([...origins]);
        if (f.rejectRemove) return false;
        for (const origin of origins) f.grants.delete(origin);
        chrome.permissions.onRemoved.emit({origins}); return true;
      }
    },
    tabs: {query: async () => [{id: 1}]},
    webNavigation: {
      getAllFrames: async () => { if (f.onFrames) await f.onFrames(); return f.frames; },
      getFrame: async ({frameId}) => { if (f.onFrame) await f.onFrame(); return f.frames.find(frame => frame.frameId === frameId); }
    },
    scripting: {executeScript: async options => {
      f.executions++;
      assert.equal(options.world, "ISOLATED");
      assert.deepEqual(Array.from(options.target.documentIds), [f.expectedDocument || DOC]);
      assert.equal(options.target.frameIds, undefined);
      assert.equal(options.func, context.magicvaultFill, "actual imported fill function");
      if (f.onExecute) await f.onExecute();
      return [{frameId: f.expectedFrame || 0, documentId: f.mismatched ? OP : (f.expectedDocument || DOC), result: {fields: ["filled"], error: null}}];
    }}
  };
  class ClockDate extends Date { static now() { return f.now; } }
  const context = vm.createContext({chrome, URL, crypto: webcrypto, Date: ClockDate});
  // Source fixtures reproduce the assembler mapping; artifact fixtures read only
  // within the actual packaged directory, so missing imports cannot be hidden.
  const load = file => fs.readFileSync(directory ? path.join(directory, file) :
    file === "fill.js" ? path.resolve(source, "../magicvault-effect/src/fill.js") : path.join(source, file), "utf8");
  context.importScripts = (...files) => {
    for (const file of files) { assert.equal(file, "fill.js"); vm.runInContext(load(file), context, {filename: file}); }
  };
  const manifest = JSON.parse(load("manifest.json"));
  assert.equal(manifest.background.type, undefined, "classic worker uses importScripts");
  assert(manifest.permissions.includes("storage"));
  vm.runInContext(load(manifest.background.service_worker), context, {filename: manifest.background.service_worker});
  assert.equal(typeof context.magicvaultFill, "function");
  const message = (value, sender = {id: ID, url: `chrome-extension://${ID}/options.html`}) => new Promise(resolve => {
    const returns = chrome.runtime.onMessage.emit(value, sender, resolve);
    if (!returns.includes(true)) resolve(undefined);
  });
  Object.defineProperty(f, "port", {get: () => port});
  Object.assign(f, {chrome, context, load, message,
    ui: (action, sender) => message({action}, sender),
    async connect() { await f.ui("connect"); port.onMessage.emit({kind: "ready", version: 2, browser_handle: OP}); await tick(); },
    deny() { f.permission = false; chrome.permissions.onRemoved.emit({origins: [...f.grants]}); },
    mismatch() { f.mismatched = true; },
    block: (site, blocked = true) => message({action: "site-block", site, blocked})
  });
  return f;
}
function options(f) {
  const elements = new Map();
  const node = () => ({value: "", textContent: "", disabled: false, dataset: {}, children: [], listeners: {},
    addEventListener(name, fn) { this.listeners[name] = fn; },
    replaceChildren() { this.children = []; this.value = ""; },
    append(child) { this.children.push(child); }});
  for (const match of f.load("options.html").matchAll(/id="([^"]+)"/g)) elements.set(match[1], node());
  const window = node();
  const document = {...node(), visibilityState: "visible", getElementById: id => { assert(elements.has(id), `missing HTML element ${id}`); return elements.get(id); }, createElement: () => node()};
  const chrome = {...f.chrome, runtime: {...f.chrome.runtime, sendMessage: f.message}};
  vm.runInNewContext(f.load("options.js"), {chrome, document, window, URL}, {filename: "options.js"});
  return {document, window, element: id => document.getElementById(id), async click(id) {
    f.gesture = true;
    let pending;
    try { pending = document.getElementById(id).listeners.click(); } finally { f.gesture = false; }
    await pending; await tick();
  }};
}
async function tick() { for (let i = 0; i < 12; i++) await new Promise(resolve => setImmediate(resolve)); }
function fill() { return {request_id: OP, request: {method: "fill", params: {target: {tab: "1", frame: "0", document: DOC, top_document: DOC, origin: "https://example.com", top_origin: "https://example.com", is_main_frame: true}, fields: [{css: "#password", value: "SYNTHETIC-WORKER-CANARY"}]}}}; }
module.exports = {fixture, options, tick, fill, ID, DOC, OP, FRAME};
