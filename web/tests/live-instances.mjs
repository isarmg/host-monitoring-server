import assert from "node:assert/strict";
import { chromium, firefox, expect, request } from "@playwright/test";
import { randomBytes, randomUUID, createHash } from "node:crypto";
import { withLocalServer } from "./local-server.mjs";

for (const engine of [chromium, firefox]) {
await withLocalServer({ prefix: "XSOS", binary: process.env.HOST_TEST_SERVER_BINARY ?? "../target/x86_64-unknown-linux-gnu/debug/xsos",
  service: "xsos",
  releaseRoot: process.env.HOST_TEST_RELEASE_ROOT,
  extraEnv: { XSOS_DEVELOPMENT: "true", XSOC_AUTHORIZATION_KEY: randomBytes(32).toString("base64") } }, async ({ base, password }) => {
  const browser = await engine.launch();
  try {
    const page = await browser.newPage({ locale: "zh-CN" }); const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    const polling = randomBytes(32).toString("base64url");
    const pairingResponse = await fetch(base + "/api/v1/xsoc/pairing-requests", {
      method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({
        protocol_version: 1, mode: "fresh",
        host: { id: randomUUID(), os: "linux", os_version: "test", kernel_version: "test", arch: "x86_64", client_version: "0.11.2" },
        token_hash: randomBytes(32).toString("hex"), polling_secret_hash: createHash("sha256").update(polling).digest("hex"),
      }),
    });
    assert.ok(pairingResponse.ok, "Pairing admission: "+(pairingResponse.ok?"ok":(await pairingResponse.json()).code));
    const pairing = await pairingResponse.json();
    const activationUrl = new URL(pairing.activation_url, base);
    assert.equal(activationUrl.origin, base);
    assert.equal((await page.goto(activationUrl.href)).status(), 200);
    await page.getByLabel("用户名", { exact: true }).fill("admin");
    await page.getByLabel("密码", { exact: true }).fill(password);
    await page.getByRole("button", { name: "登录", exact: true }).click();
    await expect(page.getByLabel("配对请求标识")).toHaveValue(pairing.request_id);
    await page.getByRole("dialog").getByRole("button", { name: "取消", exact: true }).click();
    await page.getByRole("button", { name: "新建实例", exact: true }).click();
    await expect(page.getByRole("dialog", { name: "新建 客户端 实例" })).toHaveCount(0);
    await page.getByRole("link", { name: "选择实例 新实例", exact: true }).click();
    const codeField = page.getByRole("region", { name: "配对账户信息" })
      .locator("dt").filter({ hasText: /^密码$/ }).locator("xpath=following-sibling::dd[1]").locator("code");
    await expect(codeField).toHaveText(/^[a-z0-9]{36}$/);
    const code = await codeField.innerText();
    assert.match(code, /^[a-z0-9]{36}$/);
    await page.goto(activationUrl.href);
    await page.getByRole("button", { name: "读取配对请求" }).click();
    await expect(page.getByRole("region", { name: "待核对设备" })).toContainText("linux / x86_64");
    await page.getByLabel("密码", { exact: true }).fill(code);
    await page.getByRole("button", { name: "确认设备并激活" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(page.getByRole("link", { name: "选择实例 新实例", exact: true })).toBeVisible();
    await page.getByRole("link", { name: "选择实例 新实例", exact: true }).click();
    await page.getByRole("navigation", { name: "设备信息分类" }).getByRole("button", { name: "实例概览", exact: true }).click();
    await page.getByLabel("实例名称", { exact: true }).fill("真实后端测试主机");
    await page.getByRole("button", { name: "保存名称", exact: true }).click();
    await page.getByRole("button", { name: "实例列表", exact: true }).click();
    const poll = await fetch(`${base}/api/v1/xsoc/pairing-requests/${pairing.request_id}/status`, { method: "POST", headers: { authorization: `Pairing ${polling}` } });
    assert.equal(poll.status, 200);
    assert.equal((await poll.json()).status, "active");
    await page.reload();
    await expect(page.getByRole("button", { name: "邀请与配对管理", exact: true })).toHaveCount(0);
    await expect(page.getByRole("cell", { name: "已配对", exact: true })).toBeVisible();
    assert.equal(await page.getByLabel("密码").count(), 0);
    await page.getByRole("button", { name: "新建实例", exact: true }).click();
    await expect(page.getByRole("dialog", { name: "新建 客户端 实例" })).toHaveCount(0);
    await page.getByRole("row").filter({ hasText: "待配对" }).getByRole("button", { name: "取消配对", exact: true }).click();
    await page.getByRole("button", { name: "确认", exact: true }).click();
    await expect(page.getByRole("cell", { name: "已取消", exact: true })).toBeVisible();
    await page.getByRole("link", { name: "选择实例 真实后端测试主机", exact: true }).click();
    await page.getByRole("navigation", { name: "设备信息分类" }).getByRole("button", { name: "实例概览", exact: true }).click();
    await expect(page.getByLabel("实例名称", { exact: true })).toHaveValue("真实后端测试主机");
    await expect(page.getByRole("region", { name: "详细信息与设置" })).toBeVisible();
    await expect(page.getByRole("navigation", { name: "设备信息分类" })).toBeVisible();
    await expect(page.getByRole("region", { name: "实例概览", exact: true }).first()).toBeVisible();
    await page.getByLabel("实例名称", { exact: true }).fill("修改后的监控实例");
    await page.getByRole("button", { name: "保存名称", exact: true }).click();
    await expect(page.getByLabel("实例名称", { exact: true })).toHaveValue("修改后的监控实例");
    await page.getByRole("button", { name: "实例列表", exact: true }).click();
    await expect(page.getByRole("link", { name: "选择实例 修改后的监控实例", exact: true })).toBeVisible();
    await page.getByRole("link", { name: "选择实例 修改后的监控实例", exact: true }).click();
    await page.getByRole("button", { name: "切换到深色模式", exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await page.getByRole("button", { name: "切换到浅色模式", exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await page.screenshot({ path: `/tmp/host-instance-workspace-${engine.name()}.png`, fullPage: true });
    await page.getByRole("navigation", { name: "设备信息分类" }).getByRole("button", { name: "实例概览", exact: true }).click();
    await page.getByRole("button", { name: "删除实例", exact: true }).click();
    await page.getByRole("button", { name: "确认", exact: true }).click();
    await expect(page.getByRole("link", { name: "选择实例 修改后的监控实例", exact: true })).toHaveCount(0);
    // Exercise real keyset pages and a focused detail beyond the first page.
    const session = await (await page.request.get(base + "/api/v1/auth/session")).json();
    assert.equal(typeof session.csrf_token, "string");
    const createdIdentities = [];
    const creationDeadline = Date.now() + 6 * 60_000;
    for (let index = 0; index < 55; index++) {
      let response;
      for (let attempt = 0; attempt < 3; attempt++) {
        // Each fixture request owns a new connection. The real production
        // connection lifetime is shorter than this rate-limited fixture run;
        // never retry an ambiguous failed POST on an expired keepalive socket.
        const fixtureRequest = await request.newContext({ storageState: await page.context().storageState() });
        let phase = "post";
        try {
          response = await fixtureRequest.post(base + "/api/v1/monitoring/client-instances", {
            headers: { "x-csrf-token": session.csrf_token, origin: base, "sec-fetch-site": "same-origin" },
            data: { display_name: "Pagination-" + String(index).padStart(2, "0") },
          });
          // Materialize response data before disposing its request context.
          phase = "body";
          const status = response.status(); const headers = response.headers(); const body = await response.json();
          response = { status: () => status, headers: () => headers, json: async () => body };
        } catch (error) {
          const failure = ["ECONNRESET", "Timeout", "Unexpected end", "Unexpected token"].find(value => error.message.includes(value)) ?? error.name;
          const readiness = await fetch(base + "/readyz", { signal: AbortSignal.timeout(2000) }).then(value => value.status).catch(() => 0);
          console.error(JSON.stringify({ engine: engine.name(), index, attempt, phase, failure, readiness }));
          throw new Error("A fixture invitation POST failed before an HTTP response; no ambiguous retry was attempted");
        } finally { await fixtureRequest.dispose(); }
        if (response.status() !== 429) break;
        const envelope = await response.json();
        assert.equal(envelope.code, "too_many_requests");
        assert.equal(envelope.retryable, true);
        const delay = Number(response.headers()["retry-after"]);
        assert.ok(Number.isSafeInteger(delay) && delay >= 1 && delay <= 10);
        assert.equal(envelope.details.retry_after_seconds, delay);
        assert.ok(Date.now() + delay * 1000 < creationDeadline, "Test invite creation exceeded its total budget");
        await new Promise(done => setTimeout(done, delay * 1000));
      }
      assert.equal(response.status(), 201);
      createdIdentities.push((await response.json()).request_id);
      if ((index + 1) % 10 === 0) console.log(`${engine.name()}: admitted ${index + 1} test invitations with production rate limits`);
    }
    await page.getByRole("group", { name: "全局操作" }).getByRole("button", { name: "刷新", exact: true }).click();
    const instances = page.getByRole("table", { name: "实例列表", exact: true });
    const pages = page.getByRole("navigation", { name: "实例分页", exact: true });
    await expect(instances.locator("tbody tr")).toHaveCount(50);
    const firstNames = await instances.getByRole("link").allTextContents();
    await pages.getByRole("button", { name: "下一页", exact: true }).click();
    await expect(instances.locator("tbody tr")).toHaveCount(6);
    const secondNames = await instances.getByRole("link").allTextContents();
    assert.ok(firstNames.every(name => !secondNames.includes(name)));
    await expect(pages.getByRole("button", { name: "下一页", exact: true })).toBeDisabled();
    await pages.getByRole("button", { name: "上一页", exact: true }).click();
    await expect(instances.locator("tbody tr")).toHaveCount(50);
    assert.deepEqual(await instances.getByRole("link").allTextContents(), firstNames);
    await page.goto(`${base}/#details/${createdIdentities[54]}`);
    await expect(page.getByRole("region", { name: "配对账户信息" })).toContainText("Pagination-54");
    await page.reload();
    await expect(page.getByRole("region", { name: "配对账户信息" })).toContainText("Pagination-54");
    await page.getByRole("button", { name: "日志", exact: true }).click();
    await expect(page.getByText("所选实例尚无上报记录。", { exact: true })).toBeVisible();
    assert.deepEqual(errors, []);
    console.log(`${engine.name()}: real Host backend pairing/CRUD/theme, 56 instances in 50+6 inverse pages and focused page-two detail/deep link/logs passed`);
  } finally { await browser.close(); }
});
}
