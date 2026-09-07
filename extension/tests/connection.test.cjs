const test = require("node:test");
const assert = require("node:assert/strict");
const {fixture, options, tick, fill, OP, ID} = require("./harness.cjs");
const RETRY = "magicvault-reconnect";
async function fire(f, name = RETRY) {
  const alarm = f.alarms.get(name);
  if (alarm?.when) f.now = alarm.when;
  f.alarms.delete(name);
  f.chrome.alarms.onAlarm.emit({name}); await tick();
}

test("startup connects once, persists a distinct profile capability, and never projects it to setup", async () => {
  const a = fixture(), b = fixture(); const ui = options(a); await tick();
  assert.equal(a.connects, 1); assert.equal(a.hellos[0].manual, false);
  assert.notEqual(a.hellos[0].profile_id, b.hellos[0].profile_id);
  assert.notEqual(a.hellos[0].capability, b.hellos[0].capability);
  assert.match(a.hellos[0].capability, /^[a-f0-9]{64}$/);
  assert.equal(ui.element("profile-id").textContent, a.hellos[0].profile_id);
  assert(!JSON.stringify(await a.ui("status")).includes(a.hellos[0].capability));
  options(a); a.chrome.runtime.onStartup.emit(); await tick(); assert.equal(a.connects, 1);
  const restarted = fixture({stored: a.stored}); await tick();
  assert.deepEqual(restarted.hellos[0], a.hellos[0]);
  assert.equal(a.requests.length, 0);
});

test("transient failures back off 30s to 5min, ignoring unrelated wakes and restoring lost alarms", async () => {
  const f = fixture(); await tick();
  for (const delay of [30, 60, 120, 240, 300, 300]) {
    f.port.onDisconnect.emit(); await tick();
    const interval = f.stored.connection.retryAt - f.now;
    assert(interval >= delay * 1000 && interval < (delay + 6) * 1000);
    const count = f.connects;
    f.chrome.runtime.onStartup.emit(); await tick(); assert.equal(f.connects, count);
    const restarted = fixture({stored: f.stored}); await tick();
    assert.equal(restarted.connects, 0); assert(restarted.alarms.has(RETRY));
    await fire(f); assert.equal(f.connects, count + 1);
  }
  f.port.onMessage.emit({kind: "ready", version: 2, browser_handle: OP}); await tick();
  assert.equal(f.stored.connection.failures, 0); assert.equal(f.alarms.size, 0);
});

test("pause closes immediately, survives worker restart, and explicit resume sends a manual request", async () => {
  const f = fixture(); await f.connect();
  const old = f.port; old.onMessage.emit(fill());
  await f.ui("disconnect"); await tick(); assert.equal(f.executions, 0);
  assert.equal(f.alarms.size, 0); assert.equal(f.stored.connection.mode, "paused");
  old.onDisconnect.emit(); old.onMessage.emit({kind: "ready", version: 2, browser_handle: OP});
  const restarted = fixture({stored: f.stored}); await tick();
  await fire(restarted); assert.equal(restarted.connects, 0);
  await restarted.ui("connect"); assert.equal(restarted.connects, 1);
  assert.equal(restarted.hellos[0].manual, true);
  assert.equal(restarted.hellos[0].profile_id, f.hellos[0].profile_id);
});

test("refusals, upgrades and protocol errors stop automatic retries, including across restarts", async () => {
  for (const code of ["denied", "unauthorized", "conflict", "capacity", "unsupported_version", "invalid_request", "expired", "cancelled", "persistence_uncertain"]) {
    const f = fixture(); await tick(); const old = f.port;
    old.onMessage.emit({kind: "error", version: 2, code}); old.onDisconnect.emit(); await tick();
    assert.equal(f.stored.connection.mode, "paused", code); assert.equal(f.alarms.size, 0);
    const restarted = fixture({stored: f.stored}); await tick(); assert.equal(restarted.connects, 0);
    assert.equal((await restarted.ui("status")).reason, code);
    await restarted.ui("connect"); assert.equal(restarted.hellos[0].manual, true);
  }
});

test("busy is transient but old-wire greetings and handshake timeout are terminal", async () => {
  const busy = fixture(); await tick();
  busy.port.onMessage.emit({kind: "error", version: 2, code: "busy"}); await tick();
  assert(busy.alarms.has(RETRY));
  const old = fixture(); await tick(); old.port.onMessage.emit({kind: "ready", version: 1, browser_handle: OP}); await tick();
  assert.equal((await old.ui("status")).reason, "unsupported_version");
  const hanging = fixture(); await tick(); await fire(hanging, "magicvault-handshake");
  assert.equal((await hanging.ui("status")).reason, "expired");
  assert.equal(hanging.alarms.size, 0);
});

test("stale native callbacks cannot close a replacement connection or replay a dispatched fill", async () => {
  const f = fixture(); await f.connect(); const old = f.port;
  let release; f.onExecute = () => new Promise(resolve => { release = resolve; });
  old.onMessage.emit(fill()); await tick(); assert.equal(f.executions, 1);
  old.onDisconnect.emit(); await tick(); await fire(f);
  const handle = "99999999-9999-4999-8999-999999999999";
  f.port.onMessage.emit({kind: "ready", version: 2, browser_handle: handle}); await tick();
  old.onMessage.emit({kind: "error", version: 2, code: "denied"}); old.onDisconnect.emit();
  release(); await tick();
  assert.equal(f.executions, 1); assert.equal(f.sent.length, 0);
  assert.equal((await f.ui("status")).browser_handle, handle);
});

test("a queued failure cannot override a newer pause or manual reconnect", async () => {
  const f = fixture(); await tick();
  const resume = f.ui("connect"); f.port.onDisconnect.emit(); await resume; await tick();
  assert.equal(f.connects, 2); assert.equal((await f.ui("status")).connecting, true);
  f.port.onDisconnect.emit(); await f.ui("disconnect"); await tick();
  assert.equal(f.stored.connection.reason, "paused"); assert.equal(f.alarms.size, 0);
});

test("corrupt/unwritable connection storage never regenerates an identity or opens a host", async () => {
  for (const stored of [{connection: null}, {connection: {version: 99}}]) {
    const f = fixture({stored}); await tick(); await f.ui("connect");
    assert.equal(f.connects, 0); assert.equal((await f.ui("status")).reason, "storage_unavailable");
    assert.deepEqual(f.stored, stored);
  }
  const f = fixture(); f.writeError = true; await tick(); assert.equal(f.connects, 0);
  assert.equal(f.alarms.size, 0);
});

test("website/content-script controls cannot pause, resume or inspect the profile capability", async () => {
  const f = fixture(); await tick();
  for (const action of ["connect", "disconnect", "status"]) {
    assert.equal(await f.ui(action, {id: ID, url: "https://example.com"}), undefined);
  }
  assert.equal(f.connects, 1); assert.equal(f.stored.connection.mode, "enabled");
});

test("setup follows ready, retry and paused states without manual refresh and hides capabilities", async () => {
  const f = fixture(); const ui = options(f); await tick();
  assert.match(ui.element("status").textContent, /Connecting/);
  f.port.onMessage.emit({kind: "ready", version: 2, browser_handle: OP}); await tick();
  assert.match(ui.element("status").textContent, /Connected/);
  f.port.onDisconnect.emit(); await tick(); assert.match(ui.element("status").textContent, /Automatic retry/);
  await ui.click("disconnect"); assert.match(ui.element("status").textContent, /Paused by you/);
  assert(!ui.element("status").textContent.includes(f.hellos[0].capability));
});
