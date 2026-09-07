const test = require("node:test");
const assert = require("node:assert/strict");
const {fixture, options, tick, ID} = require("./harness.cjs");

test("identity chip displays the installed runtime ID, with keyboard access and no setup side effects", async () => {
  const f = fixture(); const ui = options(f); await tick();
  assert.equal(ui.element("extension-id").textContent, ID);
  assert.match(f.load("options.html"), /class="identity-chip"/);
  assert.match(f.load("options.html"), /id="extension-id" tabindex="0" aria-label="Installed extension ID"/);
  assert.equal(f.connects, 1); assert.equal(f.requests.length, 0); assert.equal(f.writes.length, 0);
});

test("setup never auto-grants permissions or duplicates the worker connection, and existing grants remain visible", async () => {
  const f = fixture(); const ui = options(f); await tick();
  assert.equal(f.requests.length, 0); assert.equal(f.connects, 1); assert.equal(f.writes.length, 0);
  assert.match(ui.element("access-mode").textContent, /Selected websites/);
  assert.match(ui.element("granted-sites").textContent, /https:\/\/example.com/);
});

test("one user gesture enables all HTTPS; denial retains selected grants", async () => {
  const f = fixture(); const ui = options(f); await tick();
  f.rejectRequest = true; await ui.click("allow-all");
  assert.match(ui.element("permission-status").textContent, /not granted/);
  assert.deepEqual([...f.grants], ["https://example.com/*"]);
  f.rejectRequest = false; await ui.click("allow-all");
  assert.deepEqual(f.requests.at(-1), ["https://*/*"]);
  assert.equal(ui.element("access-mode").textContent, "All HTTPS websites enabled");
  assert.equal(ui.element("access-summary").dataset.state, "all");
  assert.equal(ui.element("allow-all").textContent, "All HTTPS websites enabled");
  assert.equal(ui.element("allow-all").disabled, true);
  assert(![...f.grants].some(site => site.startsWith("http:")));
});

test("switching to selected sites clears HTTPS, retains local HTTP and blocks, then allows a site", async () => {
  const f = fixture({grants: ["https://*/*", "https://example.com/*", "http://localhost/*"]});
  await f.block("https://example.com"); const ui = options(f); await tick();
  await ui.click("selected-only");
  assert.deepEqual([...f.grants], ["http://localhost/*"]);
  assert.deepEqual(f.stored.sitePolicy.blockedSites, ["https://example.com"]);
  assert.match(ui.element("permission-status").textContent, /HTTPS grants cleared/);
  ui.element("origin").value = "https://accounts.example.com:8443"; await ui.click("allow");
  assert.deepEqual(f.requests.at(-1), ["https://accounts.example.com/*"]);
  assert.match(ui.element("access-mode").textContent, /Selected websites/);
  assert.equal(ui.element("allow-all").disabled, false);
  assert.equal(ui.element("allow-all").textContent, "Allow all HTTPS websites");
});

test("failed revocation is not represented as selected-only success", async () => {
  const f = fixture({grants: ["https://*/*"]}); const ui = options(f); await tick();
  f.rejectRemove = true; await ui.click("selected-only");
  assert.match(ui.element("permission-status").textContent, /Change failed/);
  assert.match(ui.element("access-mode").textContent, /All HTTPS/);
});

test("removing one site under a broad grant directs user to block, never falsely claims revocation", async () => {
  const f = fixture({grants: ["https://*/*"]}); const ui = options(f); await tick();
  ui.element("origin").value = "https://example.com"; await ui.click("remove");
  assert.equal(f.removals.length, 0); assert.match(ui.element("permission-status").textContent, /Use Disable site/);
  await ui.click("block"); assert.deepEqual(f.stored.sitePolicy.blockedSites, ["https://example.com"]);
  assert.equal(ui.element("blocked-sites").children[0].textContent, "https://example.com");
  await ui.click("allow"); assert.deepEqual(f.stored.sitePolicy.blockedSites, ["https://example.com"]);
  ui.element("blocked-sites").value = "https://example.com";
  await ui.click("unblock"); assert.deepEqual(f.stored.sitePolicy.blockedSites, []);
});

test("local HTTP is a separate explicit grant and can be removed under all-HTTPS mode", async () => {
  const f = fixture({grants: ["https://*/*"]}); const ui = options(f); await tick();
  ui.element("origin").value = "http://127.0.0.1:8080"; await ui.click("allow");
  assert.deepEqual(f.requests.at(-1), ["http://127.0.0.1/*"]);
  await ui.click("remove"); assert.deepEqual([...f.grants], ["https://*/*"]);
});

test("invalid origins never request access; policy failures remain visible", async () => {
  const f = fixture(); const ui = options(f); await tick();
  for (const origin of ["http://example.com", "https://user:secret@example.com", "https://example.com?secret=1", "https://*.example.com", "file:///tmp/form"]) {
    ui.element("origin").value = origin; await ui.click("allow");
    assert.match(ui.element("permission-status").textContent, /Change failed/);
  }
  assert.equal(f.requests.length, 0);
  f.readError = true; await ui.click("refresh-access");
  assert.match(ui.element("block-status").textContent, /Blocklist unavailable/);
  ui.element("origin").value = "https://example.com"; await ui.click("block");
  assert.match(ui.element("permission-status").textContent, /Change failed/);
  assert.equal(f.writes.length, 0);
});

test("multiple setup pages refresh from worker-owned persistent policy and Chrome permission events", async () => {
  const f = fixture(); const first = options(f); const second = options(f); await tick();
  first.element("origin").value = "https://example.com"; await first.click("block");
  assert.equal(second.element("blocked-sites").children[0].value, "https://example.com");
  await first.click("allow-all"); assert.match(second.element("access-mode").textContent, /All HTTPS/);
});

test("pending Chrome permission request has explicit status and prevents duplicate actions", async () => {
  const f = fixture(); const ui = options(f); await tick(); let release;
  f.onRequest = () => new Promise(resolve => { release = resolve; });
  const pending = ui.click("allow-all"); await tick();
  assert.match(ui.element("permission-status").textContent, /approve or dismiss/);
  assert.equal(ui.element("allow-all").disabled, true);
  await ui.click("allow-all"); assert.equal(f.requests.length, 1);
  release(); await pending;
  assert.equal(ui.element("allow-all").disabled, true);
  assert.equal(ui.element("selected-only").disabled, false);
  assert.match(ui.element("permission-status").textContent, /All HTTPS access granted/);
});

test("confirmed all-HTTPS state is restored when setup reopens, without a new request", async () => {
  const f = fixture({grants: ["https://*/*"]});
  for (let i = 0; i < 2; i++) {
    const ui = options(f); await tick();
    assert.equal(ui.element("access-mode").textContent, "All HTTPS websites enabled");
    assert.equal(ui.element("access-summary").dataset.state, "all");
    assert.equal(ui.element("allow-all").disabled, true);
    assert.equal(ui.element("selected-only").disabled, false);
  }
  assert.equal(f.requests.length, 0);
});

test("no grants has its own indicator; denial never claims all-HTTPS enabled", async () => {
  const f = fixture({grants: []}); const ui = options(f); await tick();
  assert.equal(ui.element("access-mode").textContent, "No websites allowed yet");
  assert.equal(ui.element("access-summary").dataset.state, "none");
  f.rejectRequest = true; await ui.click("allow-all");
  assert.equal(ui.element("access-summary").dataset.state, "none");
  assert.equal(ui.element("allow-all").disabled, false);
});

test("slow or failed blocklist loading cannot hide confirmed browser access", async () => {
  const f = fixture({grants: ["https://*/*"]}); let release;
  f.onRead = () => new Promise(resolve => { release = resolve; });
  const ui = options(f); await tick();
  assert.equal(ui.element("access-summary").dataset.state, "all");
  assert.equal(ui.element("allow-all").disabled, true);
  release(); await tick(); f.onRead = undefined; f.readError = true;
  await ui.click("refresh-access");
  assert.equal(ui.element("access-mode").textContent, "All HTTPS websites enabled");
  assert.match(ui.element("block-status").textContent, /Blocklist unavailable/);
});

test("failed permission reads clear a stale enabled indicator and recover on refresh", async () => {
  const f = fixture({grants: ["https://*/*"]}); const ui = options(f); await tick();
  const getAll = f.chrome.permissions.getAll;
  f.chrome.permissions.getAll = async () => { throw new Error("SYNTHETIC-ERROR"); };
  await ui.click("refresh-access");
  assert.equal(ui.element("access-summary").dataset.state, "unknown");
  assert.equal(ui.element("access-mode").textContent, "Browser access could not be verified");
  assert.equal(ui.element("allow-all").textContent, "Allow all HTTPS websites");
  assert.doesNotMatch(ui.element("granted-sites").textContent, /https:\/\/\*/);
  f.chrome.permissions.getAll = getAll; await ui.click("refresh-access");
  assert.equal(ui.element("access-summary").dataset.state, "all");
});

test("external grants and revocations update the indicator, including focus and visibility refresh", async () => {
  const f = fixture({grants: []}); const ui = options(f); await tick();
  f.grants.add("https://*/*"); f.chrome.permissions.onAdded.emit({origins: ["https://*/*"]}); await tick();
  assert.equal(ui.element("access-summary").dataset.state, "all");
  f.grants.clear(); f.chrome.permissions.onRemoved.emit({origins: ["https://*/*"]}); await tick();
  assert.equal(ui.element("access-summary").dataset.state, "none");
  f.grants.add("https://*/*"); ui.window.listeners.focus(); await tick();
  assert.equal(ui.element("access-summary").dataset.state, "all");
  f.grants.clear(); ui.document.listeners.visibilitychange(); await tick();
  assert.equal(ui.element("access-summary").dataset.state, "none");
  assert.equal(ui.element("allow-all").disabled, false);
});

test("late permission reads cannot replace a newer confirmed access state", async () => {
  const f = fixture({grants: []}); const contains = f.chrome.permissions.contains; let release; let first = true;
  f.chrome.permissions.contains = args => {
    if (first) { first = false; return new Promise(resolve => { release = () => resolve(false); }); }
    return contains(args);
  };
  const ui = options(f); await tick();
  f.grants.add("https://*/*"); f.chrome.permissions.onAdded.emit({origins: ["https://*/*"]}); await tick();
  assert.equal(ui.element("access-summary").dataset.state, "all");
  release(); await tick();
  assert.equal(ui.element("access-mode").textContent, "All HTTPS websites enabled");
  assert.equal(ui.element("allow-all").disabled, true);
});
