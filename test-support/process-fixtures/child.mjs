// Synthetic destination contract for future process adapters, NOT MagicVault
// secure_new_process. No real credentials, shell, network, files or child spawn.
const CANARY = "SYNTHETIC-PROCESS-QUALIFICATION-CANARY";
const ROTATED = "SYNTHETIC-PROCESS-ROTATED-CANARY";
const mode = process.argv.length === 3 ? process.argv[2] : "invalid";
const reply = value => process.stdout.write(`${JSON.stringify(value)}\n`);
if (mode === "env") {
  reply({via: "env", matched: process.env.MAGICVAULT_FIXTURE_SECRET === CANARY});
} else if (mode === "echo") {
  // Intentionally adversarial recipient output for a FUTURE mediation test.
  // This literal is public synthetic data, never a caller's credential.
  process.stdout.write(`${CANARY}\n`);
  process.stderr.write(`${CANARY}\n`);
} else if (mode === "stdin" || mode === "stateful") {
  let buffer = Buffer.alloc(0);
  let current = "none";
  let generation = 0;
  let closed = false;
  const fail = () => { if (!closed) reply({error: "invalid_input"}); closed = true; process.stdin.destroy(); process.exitCode = 2; };
  const handle = text => {
    let request;
    try { request = JSON.parse(text); } catch (_) { fail(); return; }
    if (!request || typeof request !== "object" || Array.isArray(request)) { fail(); return; }
    if (request.op === "set" && typeof request.value === "string" && Object.keys(request).length === 2) {
      current = request.value === CANARY ? "initial" : request.value === ROTATED ? "rotated" : "other";
      request.value = "";
      generation++;
      reply({accepted: true, generation});
    } else if (request.op === "status" && Object.keys(request).length === 1) {
      reply({matched: current === "initial" || current === "rotated", rotated: current === "rotated", generation});
    } else if (request.op === "shutdown" && Object.keys(request).length === 1) {
      reply({stopped: true}); closed = true; process.stdin.destroy();
    } else { fail(); }
  };
  process.stdin.on("data", chunk => {
    if (closed) return;
    if (buffer.length + chunk.length > 4096) { buffer.fill(0); fail(); return; }
    const previous = buffer;
    buffer = Buffer.concat([buffer, chunk]); previous.fill(0);
    if (mode === "stateful") {
      let end;
      while (!closed && (end = buffer.indexOf(10)) !== -1) {
        const line = buffer.subarray(0, end).toString("utf8");
        const remaining = Buffer.from(buffer.subarray(end + 1));
        buffer.fill(0); buffer = remaining;
        handle(line);
      }
    }
  });
  process.stdin.on("end", () => {
    if (!closed && mode === "stdin") reply({via: "stdin", matched: buffer.toString("utf8") === CANARY});
    else if (!closed && buffer.length) fail();
    buffer.fill(0);
  });
  process.stdin.on("close", () => buffer.fill(0));
} else {
  process.stderr.write("Fixture modes: env | stdin | echo | stateful\n");
  process.exitCode = 2;
}
