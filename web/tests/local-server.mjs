import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:net";
import { mkdtemp, mkdir, open, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { randomBytes } from "node:crypto";

// Only disposable test state is created or removed; never use a live product DB.
export async function withLocalServer({ prefix, binary, extraEnv = {} }, work) {
  const root = await mkdtemp(join(tmpdir(), "sarmg-instance-browser-"));
  const password = randomBytes(24).toString("base64url");
  let child; let log;
  try {
    await mkdir(join(root, "db"), { mode: 0o700 });
    const listener = createServer();
    await new Promise(done => listener.listen(0, "127.0.0.1", done));
    const port = listener.address().port;
    await new Promise(done => listener.close(done));
    const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith(prefix + "_")));
    Object.assign(env, extraEnv, {
      [`${prefix}_BIND`]: `127.0.0.1:${port}`,
      [`${prefix}_DATABASE_URL`]: `sqlite://${root}/db/app.sqlite3`,
      [`${prefix}_STATIC_DIR`]: resolve("dist"),
      [`${prefix}_BOOTSTRAP_ADMIN_USERNAME`]: "admin",
      [`${prefix}_BOOTSTRAP_ADMIN_PASSWORD`]: password,
      [`${prefix}_CLIENT_AUTHORIZATION_KEY`]: randomBytes(32).toString("base64"),
    });
    log = await open(join(root, "server.log"), "wx", 0o600);
    child = spawn(resolve(binary), ["serve"], { cwd: root, env, stdio: ["ignore", log.fd, log.fd] });
    await new Promise((done, fail) => { child.once("spawn", done); child.once("error", fail); });
    const base = `http://127.0.0.1:${port}`;
    let ready = false;
    for (let i = 0; i < 100; i++) {
      assert.equal(child.exitCode, null, `Test service stopped; inspect ${root}`);
      try { const response = await fetch(base + "/readyz", { signal: AbortSignal.timeout(1000) }); ready = response.ok && (await response.json()).ready === true; } catch { /* startup */ }
      if (ready) break;
      await new Promise(done => setTimeout(done, 100));
    }
    assert.ok(ready, `Test service did not become ready; inspect ${root}`);
    await work({ base, password });
  } finally {
    if (child?.pid && child.exitCode === null && child.signalCode === null) {
      child.kill("SIGTERM");
      await new Promise((done, fail) => {
        const timer = setTimeout(() => fail(new Error(`Test service did not stop; state retained at ${root}`)), 30_000);
        child.once("exit", () => { clearTimeout(timer); done(); });
      });
    }
    await log?.close();
    await rm(root, { recursive: true });
  }
}
