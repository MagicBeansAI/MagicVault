const test=require("node:test");
const assert=require("node:assert/strict");
const {fixture, tick, fill, ID, DOC, OP, FRAME} = require("./harness.cjs");

test("discovery projects safe origins and document IDs, never query URLs",async()=>{
  const f=fixture();await f.connect();f.port.onMessage.emit({request_id:OP,request:{method:"targets"}});await tick();
  assert.equal(f.sent[0].result.kind,"targets");assert.equal(f.sent[0].result.data[0].origin,"https://example.com");
  assert.ok(!JSON.stringify(f.sent).includes("CANARY"));
});
test("fill uses exact document in isolated world and only returns status",async()=>{
  const f=fixture();await f.connect();const request=fill();f.port.onMessage.emit(request);await tick();
  assert.equal(f.executions,1);assert.equal(f.sent[0].result.data.fields[0],"filled");
  assert.ok(!JSON.stringify(f.sent).includes("CANARY"));assert.equal(request.request.params.fields[0].value,"");
});
test("site denial and mismatched document result never become success",async()=>{
  const denied=fixture();await denied.connect();denied.deny();denied.port.onMessage.emit(fill());await tick();
  assert.equal(denied.executions,0);assert.equal(denied.sent[0].result.data.error,"permission_denied");
  const stale=fixture();await stale.connect();stale.mismatch();stale.port.onMessage.emit(fill());await tick();
  assert.equal(stale.sent[0].result.data.fields[0],"uncertain");
});
test("page-origin setup messages and disconnect races cannot launch a fill",async()=>{
  const page=fixture();page.ui("connect",{id:ID,url:"https://example.com/"});page.port.onMessage.emit(fill());await tick();assert.equal(page.executions,0);
  const f=fixture();await f.connect();f.port.onMessage.emit(fill());f.ui("disconnect");await tick();assert.equal(f.executions,0);assert.equal(f.sent.length,0);
});

test("legacy grants are preserved; broad HTTPS never authorizes local HTTP", async () => {
  const legacy = fixture(); await tick();
  assert.equal(legacy.restricted, true); assert.equal(legacy.writes.length, 0); assert.equal(legacy.requests.length, 0);
  assert.deepEqual([...legacy.grants], ["https://example.com/*"]);
  const f = fixture({grants: ["https://*/*"]}); await f.connect();
  f.frames[0].url = "http://localhost:8080/login";
  f.port.onMessage.emit({request_id: OP, request: {method: "targets"}}); await tick();
  assert.equal(f.sent[0].result.data.length, 0);
  const request = fill(); request.request.params.target.origin = request.request.params.target.top_origin = "http://localhost:8080";
  f.port.onMessage.emit(request); await tick();
  assert.equal(f.executions, 0); assert.equal(f.sent[1].result.data.error, "permission_denied");
});

test("block overrides broad grant, normalizes case/port/trailing dot, and persists across workers", async () => {
  const f = fixture({grants: ["https://*/*"]});
  assert.equal((await f.block("https://EXAMPLE.com.:8443/")).ok, true);
  assert.deepEqual(f.stored.sitePolicy.blockedSites, ["https://example.com"]);
  for (const origin of ["https://example.com", "https://example.com.:9443"]) {
    const restarted = fixture({grants: [...f.grants], stored: f.stored}); await restarted.connect();
    restarted.frames[0].url = `${origin}/login`;
    restarted.port.onMessage.emit({request_id: OP, request: {method: "targets"}}); await tick();
    assert.equal(restarted.sent[0].result.data.length, 0);
    const request = fill(); request.request.params.target.origin = request.request.params.target.top_origin = origin;
    restarted.port.onMessage.emit(request); await tick();
    assert.equal(restarted.executions, 0); assert.equal(restarted.sent[1].result.data.error, "permission_denied");
    assert.equal(request.request.params.fields[0].value, "");
  }
  assert(!JSON.stringify(f.writes).includes("CANARY"));
  assert.equal((await f.block("https://example.com", false)).ok, true);
  await f.connect(); f.port.onMessage.emit(fill()); await tick(); assert.equal(f.executions, 1);
});

test("blocked frame is hidden and unfillable; blocked top site excludes every frame", async () => {
  for (const site of ["https://example.com", "https://login.example.org"]) {
    const f = fixture({grants: ["https://*/*"]}); await f.connect();
    f.frames.push({frameId: 2, documentId: FRAME, url: "https://login.example.org/form", documentLifecycle: "active"});
    await f.block(site);
    f.port.onMessage.emit({request_id: OP, request: {method: "targets"}}); await tick();
    assert.equal(f.sent[0].result.data.length, site === "https://example.com" ? 0 : 1);
    const request = fill(); Object.assign(request.request.params.target, {frame: "2", document: FRAME, origin: "https://login.example.org", is_main_frame: false});
    f.port.onMessage.emit(request); await tick(); assert.equal(f.executions, 0);
    assert.equal(f.sent[1].result.data.error, "permission_denied");
  }
});

test("site policy does not implicitly block subdomains or another scheme", async () => {
  const f = fixture({grants: ["https://*/*", "http://localhost/*"]}); await f.connect();
  await f.block("https://example.com"); await f.block("https://localhost");
  for (const origin of ["https://sub.example.com", "http://localhost:8080"]) {
    f.frames[0].url = `${origin}/login`;
    const request = fill(); request.request.params.target.origin = request.request.params.target.top_origin = origin;
    f.port.onMessage.emit(request); await tick();
    assert.equal(f.sent.at(-1).result.data.fields[0], "filled");
  }
  assert.equal(f.executions, 2);
});

test("only exact extension setup page can mutate policy; inputs are bounded and closed", async () => {
  const f = fixture();
  for (const sender of [{id: ID, url: "https://example.com"}, {id: "b".repeat(32), url: `chrome-extension://${ID}/options.html`}, {id: ID, url: `chrome-extension://${ID}/options.html?spoof=1`}]) {
    assert.equal(await f.message({action: "site-block", site: "https://example.com", blocked: true}, sender), undefined);
  }
  for (const site of [null, "https://user:pass@example.com", "https://example.com/path", "https://example.com?token=secret", "https://*.example.com", "https://*", "http://example.com", "file:///tmp/test", "x".repeat(257)]) {
    assert.equal((await f.block(site)).ok, false);
  }
  assert.equal((await f.block("https://example.com", "true")).ok, false);
  assert.equal(f.writes.length, 0);
  const full = fixture({stored: {sitePolicy: {version: 1, blockedSites: Array.from({length: 256}, (_, i) => `https://s${i}.example.com`)}}});
  assert.equal((await full.block("https://overflow.example.com")).ok, false);
  assert.equal(full.writes.length, 0);
});

test("corrupt or unavailable policy fails closed without logging or storing payloads", async () => {
  for (const stored of [{sitePolicy: null}, {sitePolicy: {version: 99, blockedSites: []}}, {sitePolicy: {version: 1, blockedSites: ["https://*"]}}, {sitePolicy: {version: 1, blockedSites: ["https://example.com", "https://example.com"]}}]) {
    const f = fixture({stored}); await f.connect(); f.port.onMessage.emit(fill()); await tick();
    assert.equal(f.executions, 0); assert.equal(f.sent[0].result.data.error, "permission_denied");
    assert.equal((await f.ui("site-policy")).ok, false);
  }
  const f = fixture(); await f.connect(); f.readError = true; f.port.onMessage.emit(fill()); await tick();
  assert.equal(f.executions, 0); assert.equal(f.sent[0].result.data.error, "permission_denied");
  f.readError = false; await f.block("https://example.com"); f.writeError = true;
  assert.equal((await f.block("https://example.com", false)).ok, false);
  f.port.onMessage.emit(fill()); await tick(); assert.equal(f.executions, 0);
  assert(!JSON.stringify(f.sent).includes("CANARY"));
});

test("concurrent setup writers refuse contention without losing a block", async () => {
  const f = fixture(); let release;
  f.onWrite = () => new Promise(resolve => { release = resolve; });
  const first = f.block("https://example.com"); await tick();
  assert.equal((await f.block("https://second.example.com")).ok, false);
  await f.connect(); f.port.onMessage.emit(fill()); await tick(); assert.equal(f.executions, 0);
  release(); assert.equal((await first).ok, true);
  f.onWrite = undefined; assert.equal((await f.block("https://second.example.com")).ok, true);
  assert.deepEqual(f.stored.sitePolicy.blockedSites, ["https://example.com", "https://second.example.com"]);
});

test("new blocks and Chrome revocations during document checks prevent dispatch", async () => {
  for (const update of [f => f.block("https://example.com"), f => f.deny(), f => { f.permission = false; }, f => { f.stored.sitePolicy = {version: 1, blockedSites: ["https://example.com"]}; }]) {
    const f = fixture({grants: ["https://*/*"]}); await f.connect();
    f.onFrame = async () => { f.onFrame = undefined; await update(f); };
    const request = fill(); f.port.onMessage.emit(request); await tick();
    assert.equal(f.executions, 0); assert.equal(f.sent[0].result.data.error, "permission_denied");
    assert.equal(request.request.params.fields[0].value, "");
  }
});

test("discovery discards results when site policy changes during enumeration", async () => {
  const f = fixture(); await f.connect(); f.onFrames = () => f.block("https://example.com");
  f.port.onMessage.emit({request_id: OP, request: {method: "targets"}}); await tick();
  assert.equal(f.sent[0].result.kind, "error"); assert.equal(f.sent[0].result.data, "permission_denied");
});

test("a block after dispatch does not falsely claim an already delivered fill was prevented", async () => {
  const f = fixture(); await f.connect(); f.onExecute = () => f.block("https://example.com");
  f.port.onMessage.emit(fill()); await tick(); assert.equal(f.executions, 1);
  assert.equal(f.sent[0].result.data.fields[0], "filled");
  f.port.onMessage.emit(fill()); await tick(); assert.equal(f.executions, 1);
  assert.equal(f.sent[1].result.data.error, "permission_denied");
});

test("discovery reads local policy once, not once per frame", async () => {
  const f = fixture(); await f.connect();
  f.frames = Array.from({length: 100}, (_, i) => ({frameId: i, documentId: DOC, url: "https://example.com/login", documentLifecycle: "active"}));
  f.port.onMessage.emit({request_id: OP, request: {method: "targets"}}); await tick();
  assert.equal(f.sent[0].result.data.length, 100); assert.equal(f.reads, 1);
});

test("storage access restriction failure keeps worker registered but denies discovery, fills and settings", async () => {
  const f = fixture({accessError: true}); await f.connect();
  assert.equal((await f.ui("site-policy")).ok, false);
  assert.equal((await f.block("https://example.com")).ok, false);
  f.port.onMessage.emit({request_id: OP, request: {method: "targets"}}); await tick();
  assert.equal(f.sent.length, 0); // No native channel without trusted storage.
  f.port.onMessage.emit(fill()); await tick();
  assert.equal(f.sent.length, 0);
  assert.equal(f.executions, 0); assert.equal(f.writes.length, 0);
  assert.equal(f.disconnected, undefined);
  assert.equal(f.connects, 0);
  assert.equal((await f.ui("status")).reason, "storage_unavailable");
});
