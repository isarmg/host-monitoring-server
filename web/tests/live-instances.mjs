import assert from "node:assert/strict";
import { chromium, expect } from "@playwright/test";
import { randomBytes, randomUUID, createHash } from "node:crypto";
import { withLocalServer } from "./local-server.mjs";

await withLocalServer({ prefix: "HOST_MONITORING", binary: "../../target/debug/host-monitoring-server",
  extraEnv: { HOST_MONITORING_DEVELOPMENT: "true" } }, async ({ base, password }) => {
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage({ locale: "zh-CN" }); const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    const polling = randomBytes(32).toString("base64url");
    const pairingResponse = await fetch(base + "/api/v2/host-monitor/pairing-requests", {
      method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({
        host: { id: randomUUID(), os: "linux", os_version: "test", kernel_version: "test", arch: "x86_64", client_version: "0.9.4" },
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
    await page.getByRole("dialog", { name: "新建 客户端 实例" }).getByLabel("实例名称", { exact: true }).fill("真实后端测试主机");
    await page.getByRole("button", { name: "创建实例", exact: true }).click();
    await expect(page.getByRole("dialog", { name: "新建 客户端 实例" })).toHaveCount(0);
    const code = await page.locator("td code").first().textContent();
    assert.match(code, /^uci_[0-9a-f]{32}$/);
    await page.goto(activationUrl.href);
    await page.getByRole("button", { name: "读取配对请求" }).click();
    await expect(page.getByRole("region", { name: "待核对设备" })).toContainText("linux / x86_64");
    await page.getByLabel("配对码", { exact: true }).fill(code);
    await page.getByRole("button", { name: "确认设备并激活" }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(page.getByRole("button", { name: "选择实例 真实后端测试主机", exact: true })).toBeVisible();
    const poll = await fetch(`${base}/api/v2/host-monitor/pairing-requests/${pairing.request_id}/status`, { method: "POST", headers: { authorization: `Pairing ${polling}` } });
    assert.equal(poll.status, 200);
    assert.equal((await poll.json()).status, "active");
    await page.reload();
    await expect(page.getByRole("button", { name: "邀请与配对管理", exact: true })).toHaveCount(0);
    await expect(page.getByRole("cell", { name: "已配对", exact: true })).toBeVisible();
    assert.equal(await page.getByLabel("配对码").count(), 0);
    await page.getByRole("button", { name: "新建实例", exact: true }).click();
    await page.getByRole("dialog", { name: "新建 客户端 实例" }).getByLabel("实例名称", { exact: true }).fill("待取消测试实例");
    await page.getByRole("button", { name: "创建实例", exact: true }).click();
    await expect(page.getByRole("dialog", { name: "新建 客户端 实例" })).toHaveCount(0);
    await page.getByRole("button", { name: "取消配对", exact: true }).click();
    await page.getByRole("button", { name: "确认", exact: true }).click();
    await expect(page.getByRole("cell", { name: "已取消", exact: true })).toBeVisible();
    await page.getByRole("button", { name: "选择实例 真实后端测试主机", exact: true }).click();
    await expect(page.getByRole("heading", { name: "真实后端测试主机", exact: true })).toBeVisible();
    await expect(page.getByRole("region", { name: "详细信息与设置" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "最新设备信息", exact: true })).toBeVisible();
    await expect(page.getByRole("heading", { name: "历史趋势", exact: true })).toBeVisible();
    await page.getByLabel("实例名称", { exact: true }).fill("修改后的监控实例");
    await page.getByRole("button", { name: "保存设置", exact: true }).click();
    await expect(page.getByRole("heading", { name: "修改后的监控实例", exact: true })).toBeVisible();
    await page.getByRole("button", { name: "实例列表", exact: true }).click();
    await expect(page.getByRole("button", { name: "选择实例 修改后的监控实例", exact: true })).toBeVisible();
    await page.getByRole("button", { name: "选择实例 修改后的监控实例", exact: true }).click();
    await page.getByRole("button", { name: "切换到深色模式", exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await page.getByRole("button", { name: "切换到浅色模式", exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await page.screenshot({ path: "/tmp/host-instance-workspace.png", fullPage: true });
    await page.getByRole("button", { name: "删除实例", exact: true }).click();
    await page.getByRole("button", { name: "确认", exact: true }).click();
    await expect(page.getByRole("button", { name: "选择实例 修改后的监控实例", exact: true })).toHaveCount(0);
    assert.deepEqual(errors, []);
    console.log("Real Host backend: pairing/deep link/cancel/select/details/save/delete/theme toggle passed");
  } finally { await browser.close(); }
});
