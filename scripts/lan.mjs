import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { existsSync, statSync } from "node:fs";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const projectRoot = path.resolve(__dirname, "..");
const distDir = path.join(projectRoot, "dist");

const publicHost = process.env.CODEX_SWITCHER_LAN_HOST ?? "0.0.0.0";
const publicPort = Number.parseInt(process.env.CODEX_SWITCHER_LAN_PORT ?? "3210", 10);
const backendHost = "127.0.0.1";
const backendPort = Number.parseInt(process.env.CODEX_SWITCHER_WEB_PORT ?? "3211", 10);
const backendUrl = `http://${backendHost}:${backendPort}`;
const backendExe = path.join(projectRoot, "src-tauri", "target", "debug", "codex-web.exe");

if (!existsSync(backendExe)) {
  console.error(`Missing backend executable: ${backendExe}`);
  console.error("Run `cargo build --manifest-path src-tauri/Cargo.toml --bin codex-web` first.");
  process.exit(1);
}

const backend = spawn(backendExe, {
  cwd: path.join(projectRoot, "src-tauri"),
  env: {
    ...process.env,
    CODEX_SWITCHER_WEB_HOST: backendHost,
    CODEX_SWITCHER_WEB_PORT: String(backendPort),
  },
  stdio: ["ignore", "pipe", "pipe"],
  windowsHide: false,
});

backend.stdout.on("data", (chunk) => {
  process.stdout.write(`[backend] ${chunk}`);
});

backend.stderr.on("data", (chunk) => {
  process.stderr.write(`[backend] ${chunk}`);
});

backend.on("exit", (code, signal) => {
  console.error(`[backend] exited with code=${code} signal=${signal}`);
  process.exit(code ?? 1);
});

await waitForBackend();

const server = createServer(async (req, res) => {
  try {
    const url = new URL(req.url ?? "/", `http://${req.headers.host ?? `127.0.0.1:${publicPort}`}`);

    if (url.pathname.startsWith("/api/")) {
      await proxyApi(req, res, url);
      return;
    }

    await serveStatic(res, url.pathname);
  } catch (error) {
    res.writeHead(500, { "Content-Type": "text/plain; charset=utf-8" });
    res.end(error instanceof Error ? error.message : String(error));
  }
});

server.listen(publicPort, publicHost, () => {
  console.log(`Codex Switcher LAN proxy listening on http://192.168.0.119:${publicPort}`);
  console.log(`Proxying API to ${backendUrl}`);
});

const shutdown = () => {
  server.close();
  backend.kill();
};

process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);
process.on("exit", shutdown);

async function waitForBackend() {
  const deadline = Date.now() + 30000;

  while (Date.now() < deadline) {
    try {
      const response = await fetch(`${backendUrl}/api/health`);
      if (response.ok) return;
    } catch {
      // Backend is still starting.
    }

    await delay(500);
  }

  console.error(`Backend did not become ready at ${backendUrl}/api/health`);
  backend.kill();
  process.exit(1);
}

async function proxyApi(req, res, url) {
  const headers = new Headers();
  for (const [key, value] of Object.entries(req.headers)) {
    if (typeof value === "string") {
      headers.set(key, value);
    } else if (Array.isArray(value)) {
      headers.set(key, value.join(", "));
    }
  }
  headers.set("host", `${backendHost}:${backendPort}`);

  const body =
    req.method === "GET" || req.method === "HEAD"
      ? undefined
      : await readRequestBody(req);

  const response = await fetch(`${backendUrl}${url.pathname}${url.search}`, {
    method: req.method,
    headers,
    body,
  });

  const responseHeaders = {};
  for (const [key, value] of response.headers.entries()) {
    if (key === "connection" || key === "transfer-encoding") continue;
    responseHeaders[key] = value;
  }

  res.writeHead(response.status, responseHeaders);
  const buffer = Buffer.from(await response.arrayBuffer());
  res.end(buffer);
}

async function serveStatic(res, pathname) {
  const filePath = resolveStaticPath(pathname);
  const data = await readFile(filePath);
  res.writeHead(200, {
    "Content-Type": contentType(filePath),
    "Cache-Control": "no-cache",
  });
  res.end(data);
}

function resolveStaticPath(pathname) {
  const normalized = pathname === "/" ? "/index.html" : pathname;
  const stripped = normalized.replace(/^\/+/, "");
  const requestedPath = path.normalize(stripped);

  if (requestedPath.startsWith("..")) {
    return path.join(distDir, "index.html");
  }

  const candidate = path.join(distDir, requestedPath);
  const hasExtension = path.extname(candidate) !== "";

  if (existsSync(candidate) && statSync(candidate).isFile()) {
    return candidate;
  }

  if (hasExtension) {
    return path.join(distDir, "index.html");
  }

  return path.join(distDir, "index.html");
}

function contentType(filePath) {
  switch (path.extname(filePath).toLowerCase()) {
    case ".css":
      return "text/css; charset=utf-8";
    case ".html":
      return "text/html; charset=utf-8";
    case ".ico":
      return "image/x-icon";
    case ".js":
      return "text/javascript; charset=utf-8";
    case ".json":
      return "application/json; charset=utf-8";
    case ".png":
      return "image/png";
    case ".svg":
      return "image/svg+xml";
    default:
      return "application/octet-stream";
  }
}

function readRequestBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];

    req.on("data", (chunk) => {
      chunks.push(chunk);
    });

    req.on("end", () => {
      resolve(Buffer.concat(chunks));
    });

    req.on("error", reject);
  });
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
