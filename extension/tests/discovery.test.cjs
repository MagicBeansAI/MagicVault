const test = require("node:test");
const assert = require("node:assert/strict");
const {fixture, tick, DOC, OP, fill} = require("./harness.cjs");

const tab = (id, url = "https://example.com/login", extra = {}) => ({id, url, discarded: false, ...extra});
const frame = (url = "https://example.com/login", frameId = 0) =>
  ({frameId, documentId: DOC, url, documentLifecycle: "active"});
async function discover(f, filter) {
  const before = f.sent.length;
  f.port.onMessage.emit({request_id: OP, request: filter === undefined ? {method: "targets"} : {method: "filtered_targets", params: filter}});
  await tick();
  assert.equal(f.sent.length, before + 1);
  return f.sent.at(-1).result;
}

test("hundreds of unrelated tabs do not consume the permitted discovery budget", async () => {
  const f = fixture(); await f.connect();
  f.tabs = Array.from({length: 500}, (_, i) => tab(i + 2, "https://ungranted.example/login"));
  f.tabs.push(tab(1));
  const result = await discover(f);
  assert.equal(result.kind, "targets"); assert.equal(result.data.length, 1);
  assert.deepEqual(f.tabQueries, [{url: ["https://example.com/*"], discarded: false}]);
  assert.deepEqual(f.frameQueries, [1]);
  assert.equal(f.permissionChecks.length, 1);
});

test("explicit origin/tab narrowing survives an all-granted profile over capacity", async () => {
  const f = fixture({grants: ["https://*/*"]}); await f.connect();
  f.tabs = Array.from({length: 500}, (_, i) => tab(i + 2, "https://other.example/login"));
  f.tabs.push(tab(1));
  assert.equal((await discover(f)).data, "capacity");
  f.frameQueries.length = 0;
  const result = await discover(f, {top_origin: "https://example.com", tab_id: "1"});
  assert.equal(result.kind, "targets"); assert.equal(result.data.length, 1);
  assert.deepEqual(f.frameQueries, [1]);
  assert.deepEqual(f.tabQueries.at(-1), {url: ["https://example.com/*"], discarded: false});
  assert.equal((await discover(f, {tab_id: "1"})).data.length, 1);
  assert.equal((await discover(f, {tab_id: "9999"})).data.length, 0);
  assert.equal((await discover(f, {top_origin: "https://other.example", tab_id: "1"})).data.length, 0);
});

test("origin narrowing respects exact ports, grants, site blocks and navigation", async () => {
  const f = fixture({grants: ["http://127.0.0.1/*"]}); await f.connect();
  f.tabs = Array.from({length: 200}, (_, i) => tab(i + 2, "http://127.0.0.1:4444/login"));
  f.tabs.push(tab(1, "http://127.0.0.1:5555/login"));
  f.frames = [frame("http://127.0.0.1:5555/login")];
  assert.equal((await discover(f, {top_origin: "http://127.0.0.1:5555"})).data.length, 1);
  assert.deepEqual(f.frameQueries, [1]);
  f.frames = [frame("http://127.0.0.1:4444/login")];
  assert.equal((await discover(f, {top_origin: "http://127.0.0.1:5555"})).data.length, 0);
  const queries = f.tabQueries.length;
  assert.equal((await discover(f, {top_origin: "https://ungranted.example"})).data.length, 0);
  await f.block("http://127.0.0.1:5555");
  assert.equal((await discover(f, {top_origin: "http://127.0.0.1:5555"})).data.length, 0);
  assert.equal(f.tabQueries.length, queries);
});

test("invalid narrowing fails closed before browser enumeration", async () => {
  for (const filter of [null, [], "origin", {origin: "https://example.com"}, {top_origin: 1},
    {top_origin: "https://example.com/login"}, {top_origin: "https://*.example.com"},
    {top_origin: "https://example.com/"}, {top_origin: "not-a-url"}, {top_origin: "https://user:secret@example.com"},
    {tab_id: "01"}, {tab_id: "1\n"}, {tab_id: 1}, {tab_id: "-1"}, {tab_id: "1".repeat(257)}]) {
    const f = fixture(); await f.connect();
    const result = await discover(f, filter);
    assert.equal(result.kind, "error"); assert.equal(result.data, "invalid_request");
    assert.equal(f.tabQueries.length, 0); assert.equal(f.frameQueries.length, 0);
  }
});

test("revocation during filtered preflight refuses instead of publishing empty success", async () => {
  const f = fixture(); await f.connect();
  f.onContains = () => { f.onContains = undefined; f.deny(); };
  const result = await discover(f, {top_origin: "https://example.com"});
  assert.equal(result.kind, "error"); assert.equal(result.data, "permission_denied");
  assert.equal(f.tabQueries.length, 0);
});

test("empty Chrome grants return no targets without tab or frame inspection", async () => {
  const f = fixture({grants: []}); await f.connect();
  f.tabs = Array.from({length: 500}, (_, i) => tab(i + 1));
  const result = await discover(f);
  assert.equal(result.kind, "targets"); assert.equal(result.data.length, 0);
  assert.equal(f.tabQueries.length, 0); assert.equal(f.frameQueries.length, 0);
});

test("blocked, discarded, unsupported, missing-URL and invalid-ID tabs do not count", async () => {
  const f = fixture({grants: ["https://*/*"]}); await f.connect();
  await f.block("https://blocked.example");
  // Defensively handle rows even if Chrome does not apply the URL query filter.
  f.ignoreTabQuery = true;
  f.tabs = Array.from({length: 300}, (_, i) => tab(i + 2, "https://blocked.example/login"));
  f.tabs.push(...Array.from({length: 300}, (_, i) => tab(i + 400, "https://example.com", {discarded: true})));
  f.tabs.push(tab(1000, "chrome://extensions"), {id: 1001}, tab(-1), tab(1.5), tab(1));
  const result = await discover(f);
  assert.equal(result.kind, "targets"); assert.equal(result.data.length, 1);
  assert.deepEqual(f.frameQueries, [1]);
  assert.equal(f.permissionChecks.length, 1);
});

test("65 and 128 permitted loaded tabs are discoverable; 129 refuses without frame work", async () => {
  for (const count of [65, 128, 129]) {
    const f = fixture(); await f.connect();
    f.tabs = Array.from({length: count}, (_, i) => tab(i + 1));
    const result = await discover(f);
    if (count <= 128) {
      assert.equal(result.kind, "targets"); assert.equal(result.data.length, count);
      assert.equal(new Set(result.data.map(t => t.tab)).size, count);
      assert.equal(f.frameQueries.length, count);
    } else {
      assert.equal(result.kind, "error"); assert.equal(result.data, "capacity");
      assert.equal(f.frameQueries.length, 0);
    }
    assert.equal(f.permissionChecks.length, 1);
  }
});

test("the 128-target response budget still refuses overflow without partial discovery", async () => {
  const f = fixture(); await f.connect();
  f.tabs = [tab(1), tab(2)];
  f.framesByTab = new Map([[1, Array.from({length: 128}, (_, i) => frame(undefined, i))], [2, [frame()]]]);
  const result = await discover(f);
  assert.equal(result.kind, "error"); assert.equal(result.data, "capacity");
  assert.equal(f.permissionChecks.length, 1);
  assert.equal(f.executions, 0);
});

test("no inspection beyond the per-tab frame budget, but other tabs remain discoverable", async () => {
  const f = fixture(); await f.connect();
  f.tabs = [tab(1), tab(2)];
  f.framesByTab = new Map([[1, Array.from({length: 129}, (_, i) => frame(undefined, i))], [2, [frame()]]]);
  const result = await discover(f);
  assert.equal(result.kind, "targets"); assert.equal(result.data.length, 1);
  assert.equal(result.data[0].tab, "2");
});

test("navigation to an ungranted or failed top document never inherits tab-query authority", async () => {
  for (const top of [frame("https://ungranted.example/login"), {...frame(), errorOccurred: true}]) {
    const f = fixture(); await f.connect();
    f.tabs = [tab(1)]; f.frames = [top, frame(undefined, 1)];
    const result = await discover(f);
    assert.equal(result.kind, "targets"); assert.equal(result.data.length, 0);
  }
});

test("permission changes at every asynchronous enumeration boundary discard discovery", async () => {
  for (const hook of ["onGetAll", "onTabs", "onContains", "onFrames"]) {
    const f = fixture(); await f.connect();
    f[hook] = () => { f[hook] = undefined; f.deny(); };
    const result = await discover(f);
    assert.equal(result.kind, "error", hook); assert.equal(result.data, "permission_denied", hook);
    assert.equal(f.executions, 0);
  }
});

test("permission snapshot failure is a closed error, never unfiltered tab fallback", async () => {
  const f = fixture(); await f.connect(); f.getAllError = true;
  const result = await discover(f);
  assert.equal(result.kind, "error"); assert.equal(result.data, "permission_denied");
  assert.equal(f.tabQueries.length, 0); assert.equal(f.frameQueries.length, 0);
  assert(!JSON.stringify(f.sent).includes("SYNTHETIC"));
});

test("cached discovery permission does not survive a request or authorize a fill", async () => {
  const f = fixture(); await f.connect();
  assert.equal((await discover(f)).data.length, 1);
  f.permission = false; // Also check a missed/late Chrome notification.
  assert.equal((await discover(f)).data.length, 0);
  f.port.onMessage.emit(fill()); await tick();
  assert.equal(f.executions, 0);
  assert.equal(f.sent.at(-1).result.data.error, "permission_denied");
});

test("disconnect during enumeration cannot publish targets on an obsolete channel", async () => {
  const f = fixture(); await f.connect();
  f.onTabs = () => f.ui("disconnect");
  f.port.onMessage.emit({request_id: OP, request: {method: "targets"}}); await tick();
  assert.equal(f.sent.length, 0); assert.equal(f.frameQueries.length, 0);
});
