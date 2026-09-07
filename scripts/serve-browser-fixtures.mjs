// No third-party dependencies, dynamic paths, request logging, or value echo.
import {createServer} from "node:http";
import {readFileSync} from "node:fs";
import {pathToFileURL} from "node:url";

const html = readFileSync(new URL("../test-support/browser-fixtures/index.html", import.meta.url));
const javascript = readFileSync(new URL("../test-support/browser-fixtures/fixture.js", import.meta.url));
const pages = new Set(["/", "/login", "/controls", "/frames", "/frame", "/next"]);
export const CSP = "default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; frame-src 'self'; connect-src 'none'; form-action 'none'; base-uri 'none'";

export async function startFixtureServer(port = 0) {
  if (!Number.isInteger(port) || port < 0 || port > 65535) throw new Error("invalid_port");
  const server = createServer({maxHeaderSize: 4096, requestTimeout: 5000, headersTimeout: 5000}, (request, response) => {
    response.setHeader("Content-Security-Policy", CSP);
    response.setHeader("Cache-Control", "no-store");
    response.setHeader("X-Content-Type-Options", "nosniff");
    response.setHeader("Connection", "close");
    const route = request.url;
    if (!["GET", "HEAD"].includes(request.method)) {
      response.writeHead(405); response.end(); return;
    }
    const body = pages.has(route) ? html : route === "/fixture.js" ? javascript : null;
    if (!body) { response.writeHead(route === "/favicon.ico" ? 204 : 404); response.end(); return; }
    response.setHeader("Content-Type", route === "/fixture.js" ? "text/javascript; charset=utf-8" : "text/html; charset=utf-8");
    response.setHeader("Content-Length", body.length);
    response.writeHead(200);
    response.end(request.method === "HEAD" ? undefined : body);
  });
  server.maxConnections = 16;
  server.setTimeout(5000, socket => socket.destroy());
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", resolve);
  });
  return {server, origin: `http://127.0.0.1:${server.address().port}`};
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const args = process.argv.slice(2);
  if (args.length && !(args.length === 2 && args[0] === "--port" && /^\d{1,5}$/.test(args[1]))) {
    process.stderr.write("Usage: node scripts/serve-browser-fixtures.mjs [--port NUMBER]\n");
    process.exitCode = 1;
  } else {
    try {
      const {server, origin} = await startFixtureServer(args.length ? Number(args[1]) : 0);
      process.stdout.write(`Synthetic-only qualification site: ${origin}\n`);
      const stop = () => { server.closeAllConnections(); server.close(); };
      process.once("SIGINT", stop); process.once("SIGTERM", stop);
    } catch (_) {
      process.stderr.write("Fixture server unavailable. No request data was logged.\n");
      process.exitCode = 1;
    }
  }
}
