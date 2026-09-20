import assert from "node:assert/strict";
import { chromium, firefox, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { preview } from "vite";

const session = { authenticated: true, user_id: "A".repeat(43), username: "admin", role: "admin", csrf_token: "A".repeat(43) };
const inviteId = "018f1f4b-7a5d-7b5f-8d31-123456789abc";
const pairId = "018f1f4b-7a5d-7b5f-8d31-123456789abd";
const instanceId = "018f1f4b-7a5d-7b5f-8d31-123456789abe";
const code = "uci_" + "a".repeat(32);
const server = await preview({ preview: { host: "127.0.0.1", port: 0, strictPort: true } });
try {
  for (const engine of [chromium, firefox]) {
    const browser = await engine.launch();
    try {
      const context = await browser.newContext({ locale: "zh-CN",  viewport: { width: 360, height: 740 } });
      const page = await context.newPage();
      const errors = []; let invitation = null; let creates = 0; let activations = 0; let release;
      page.on("pageerror", error => errors.push(error.message));
      await page.route("**/api/v2/**", async route => {
        const request = route.request(); const path = new URL(request.url()).pathname;
        if (request.method() !== "GET") assert.equal(request.headers()["x-csrf-token"], session.csrf_token);
        if (path.endsWith("/monitoring/hosts")) return route.fulfill({ json: { hosts: [], statistics: { total: { total: 0, online: 0 }, windows: { total: 0, online: 0 }, linux: { total: 0, online: 0 }, macos: { total: 0, online: 0 } }, total: 0, limit: 50, offset: 0 } });
        if (path.endsWith("/client-instances")) {
          if (request.method() === "POST") {
            creates++; assert.deepEqual(Object.keys(request.postDataJSON()),["display_name"]);
            if (creates === 1) return route.fulfill({ status: 503, headers: { "x-request-id": "invite-123" }, json: { code: "service_unavailable", retryable: true, message: "SECRET", request_id: "invite-123" } });
            await new Promise(done => { release = done; });
            invitation = { request_id: inviteId, instance_id: instanceId, display_name: request.postDataJSON().display_name,
              status: "pending", created_at: "2026-09-05T00:00:00Z", authorization_code: code };
            return route.fulfill({ status: 201, json: { ...invitation, activation_code: code } });
          }
          return route.fulfill({ json: { instances: invitation ? [invitation] : [], hosts: [], total: invitation ? 1 : 0, limit: 50, offset: 0 } });
        }
        if (path.endsWith(`/client-instances/${inviteId}/delete`)) {
          invitation = null;
          return route.fulfill({ status: 204 });
        }
        if (path.endsWith(`/client-instances/${inviteId}`)) {
          if (invitation.status === "cancelled") invitation = null; else invitation.status = "cancelled";
          return route.fulfill({ status: 204 });
        }
        if (path.endsWith(`/pairing-requests/${pairId}`)) return route.fulfill({ json: {
          request_id: pairId, os: "linux", arch: "x86_64", client_version: "0.8.1", status: activations ? "active" : "waiting", expires_at: "2099-01-01T00:00:00Z",
        } });
        if (path.endsWith("/activate-admin")) {
          assert.deepEqual(request.postDataJSON(), { request_id: pairId, activation_code: code });
          activations++; invitation.status = "active";
          return route.fulfill({ json: { instance_id: instanceId, status: "active" } });
        }
        return route.fulfill({ json: session });
      });
      await page.goto(`http://127.0.0.1:${server.httpServer.address().port}`);
      await expect(page.getByRole("button", { name: "邀请与配对管理", exact: true })).toHaveCount(0);
      await page.getByRole("button", { name: "新建实例", exact: true }).click();
      await expect(page.getByLabel("有效期（分钟）")).toHaveCount(0);
      const instanceName = page.getByLabel("实例名称", { exact: true });
      for (const character of ["a", "中", "😀"]) {
        await instanceName.fill(character.repeat(32));
        assert.equal(await instanceName.evaluate(input => input.checkValidity()), true);
      }
      await instanceName.fill("名".repeat(33));
      assert.equal(await instanceName.evaluate(input => input.checkValidity()), false);
      await page.getByLabel("实例名称", { exact: true }).fill("测试 Client");
      await page.getByRole("button", { name: "创建实例", exact: true }).click();
      await expect(page.getByRole("alert")).toContainText("invite-123");
      await expect(page.locator("body")).not.toContainText("SECRET");
      await page.getByRole("button", { name: "创建实例", exact: true }).click();
      await expect.poll(() => typeof release).toBe("function");
      await page.getByRole("dialog").locator("form").evaluate(form => form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })));
      await page.keyboard.press("Escape"); await expect(page.getByRole("dialog")).toBeVisible();
      assert.equal(creates, 2); release();
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await expect(page.getByRole("button", { name: "关闭通知", exact: true })).toBeVisible();
      await expect(page.getByRole("button", { name: "关闭通知", exact: true })).toHaveCount(0, { timeout: 7_000 });
      await expect(page.getByRole("cell").filter({ hasText: code })).toBeVisible();
      const instanceTable = page.getByRole("table", { name: "实例列表" });
      assert.ok((await instanceTable.locator("th, td").evaluateAll(elements => elements.map(element => getComputedStyle(element).textAlign))).every(value => value === "left"));
      assert.ok((await instanceTable.locator(".sarmg-actions").evaluateAll(elements => elements.map(element => getComputedStyle(element).justifyContent))).every(value => value === "flex-start"));
      for (const theme of ["light", "dark"]) {
        await page.evaluate(theme => { document.documentElement.dataset.theme = theme; }, theme);
        assert.deepEqual((await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze()).violations, []);
      }
      assert.equal(await page.evaluate(() => JSON.stringify({ ...localStorage, ...sessionStorage }).includes("uci_")), false);
      await page.reload();
      await expect(page.getByRole("dialog", { name: "新建 客户端 实例" })).toHaveCount(0);
      await page.getByRole("button", { name: "取消配对", exact: true }).click();
      await page.getByRole("button", { name: "确认", exact: true }).click();
      await expect(page.getByRole("cell", { name: "已取消", exact: true })).toBeVisible();
      await page.getByRole("button", { name: "删除", exact: true }).click();
      await expect(page.getByRole("button", { name: "取消", exact: true })).toBeVisible();
      await page.getByRole("button", { name: "取消", exact: true }).click();
      await expect(page.getByRole("button", { name: "确认删除", exact: true })).toHaveCount(0);
      await page.getByRole("button", { name: "删除", exact: true }).click();
      await page.getByRole("button", { name: "确认删除", exact: true }).click();
      await expect(page.getByText("暂无实例", { exact: true })).toBeVisible();
      // A new trusted invitation models the independent Client pairing request.
      invitation = { request_id: inviteId, instance_id: instanceId, display_name: "测试 Client", status: "pending", created_at: "2026-09-05T00:00:00Z", authorization_code: code };
      await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/activate/${pairId}`);
      await expect(page.getByRole("dialog", { name: "激活 客户端 配对" })).toBeVisible();
      await expect(page.getByLabel("配对请求标识")).toHaveValue(pairId);
      await page.getByRole("button", { name: "读取配对请求" }).click();
      await expect(page.getByRole("region", { name: "待核对设备" })).toContainText("linux / x86_64");
      await page.getByLabel("配对码", { exact: true }).fill(code);
      await page.getByRole("button", { name: "确认设备并激活" }).click();
      await expect(page.getByRole("dialog")).toHaveCount(0);
      await expect(page.getByRole("cell", { name: "已配对", exact: true })).toBeVisible();
      assert.equal(activations, 1);
      assert.ok(!page.url().includes(code));
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
      assert.deepEqual(errors, []);
      console.log(`${engine.name()}: instance/create/immediate-close/failure/single submit/long-lived code/cancel/deep-link/device confirmation/activation passed`);
    } finally { await browser.close(); }
  }
} finally { await new Promise(done => server.httpServer.close(done)); }
