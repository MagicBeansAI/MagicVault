import test from "node:test";
import assert from "node:assert/strict";
import {spawn} from "node:child_process";
import {fileURLToPath} from "node:url";
import {startFixtureServer} from "../serve-browser-fixtures.mjs";

const CANARY = "SYNTHETIC-PROCESS-QUALIFICATION-CANARY";
const ROTATED = "SYNTHETIC-PROCESS-ROTATED-CANARY";
function child(mode, input = "", environment = {}) {
  return new Promise((resolve, reject) => {
    const processUnderTest = spawn(process.execPath, [fileURLToPath(new URL("../../test-support/process-fixtures/child.mjs", import.meta.url)), mode], {
      env: environment, stdio: ["pipe", "pipe", "pipe"],
    });
    let stdout = "", stderr = "";
    const timer = setTimeout(() => { processUnderTest.kill("SIGKILL"); reject(new Error("fixture_timeout")); }, 5000);
    processUnderTest.stdout.on("data", bytes => { stdout += bytes; });
    processUnderTest.stderr.on("data", bytes => { stderr += bytes; });
    // Refusal may close stdin before all test bytes are accepted. Still collect
    // the child's exit/status; do not turn an expected early close into an
    // unhandled stream error in the test runner.
    processUnderTest.stdin.on("error", error => {
      if (error.code !== "EPIPE" && error.code !== "ERR_STREAM_DESTROYED") {
        processUnderTest.kill("SIGKILL"); reject(error);
      }
    });
    processUnderTest.once("error", error => { clearTimeout(timer); reject(error); });
    processUnderTest.once("close", code => { clearTimeout(timer); resolve({code, stdout, stderr}); });
    processUnderTest.stdin.end(input);
  });
}
test("browser server serves only fixed loopback assets and never echoes submitted material", async () => {
  const {server, origin} = await startFixtureServer();
  try {
    const page = await fetch(`${origin}/login`);
    assert.equal(page.status, 200);
    assert.match(page.headers.get("content-security-policy"), /form-action 'none'/);
    assert.match(await page.text(), /id="password"/);
    for (const path of ["/controls", "/frames", "/frame", "/next", "/fixture.js"]) {
      const response = await fetch(origin + path); assert.equal(response.status, 200); await response.arrayBuffer();
    }
    const refused = await fetch(`${origin}/login`, {method:"POST", body:CANARY});
    assert.equal(refused.status, 405); assert.equal(await refused.text(), "");
    const query = await fetch(`${origin}/login?value=${CANARY}`);
    assert.equal(query.status, 404); assert.equal(await query.text(), "");
  } finally { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
});
test("process destination reports only booleans for synthetic env and stdin input", async () => {
  for (const result of [await child("env", "", {MAGICVAULT_FIXTURE_SECRET:CANARY}), await child("stdin", CANARY)]) {
    assert.equal(result.code, 0); assert.equal(JSON.parse(result.stdout).matched, true);
    assert.ok(!result.stdout.includes(CANARY)); assert.equal(result.stderr, "");
  }
});
test("running synthetic service accepts explicit IPC rotation without echoing values", async () => {
  const requests = [{op:"set",value:CANARY},{op:"status"},{op:"set",value:ROTATED},{op:"status"},{op:"shutdown"}];
  const result = await child("stateful", requests.map(value=>JSON.stringify(value)).join("\n")+"\n");
  assert.equal(result.code, 0); assert.equal(result.stderr, "");
  const replies = result.stdout.trim().split("\n").map(text=>JSON.parse(text));
  assert.deepEqual(replies[3], {matched:true,rotated:true,generation:2}); assert.deepEqual(replies[4], {stopped:true});
  assert.ok(!result.stdout.includes(CANARY)); assert.ok(!result.stdout.includes(ROTATED));
});
test("adversarial echo probe emits only its fixed public canary for future mediation qualification", async () => {
  const result = await child("echo"); assert.equal(result.code, 0);
  assert.equal(result.stdout.trim(), CANARY); assert.equal(result.stderr.trim(), CANARY);
});
test("process fixture refuses oversized input and unknown commands without reflection", async () => {
  for (const result of [await child("stdin", "x".repeat(4097)), await child("stateful", JSON.stringify({op:"unknown",value:CANARY})+"\n")]) {
    assert.equal(result.code, 2); assert.ok(!result.stdout.includes(CANARY)); assert.equal(result.stderr, "");
  }
});
