import assert from "node:assert/strict";
import { chromium, firefox, expect } from "@playwright/test";
import { withLocalServer } from "./local-server.mjs";

for (const engine of [chromium, firefox]) {
  await withLocalServer({
    prefix: "XSOS", service: "xsos",
    binary: process.env.HOST_TEST_SERVER_BINARY ?? "../target/x86_64-unknown-linux-gnu/debug/xsos",
    extraEnv: { XSOS_DEVELOPMENT: "true" },
  }, async ({ base, password }) => {
    const browser = await engine.launch();
    const context = await browser.newContext({ locale: "zh-CN", viewport: { width: 360, height: 800 } });
    const other = await browser.newContext();
    try {
      const page = await context.newPage(); page.setDefaultTimeout(30_000);
      const errors = []; page.on("pageerror", error => errors.push(error.message));
      const headers = { origin: base, "sec-fetch-site": "same-origin" };
      assert.equal((await other.request.post(base + "/api/v1/auth/login", { headers, data: { username: "admin", password } })).status(), 200);
      await page.goto(base);
      await page.getByLabel("用户名", { exact: true }).fill("admin");
      await page.getByLabel("密码", { exact: true }).fill(password);
      await page.getByRole("button", { name: "登录", exact: true }).click();
      const entry = page.getByRole("banner").getByRole("button", { name: "账号设置", exact: true });
      await expect(entry).toBeVisible();
      const originalCookies = await context.cookies();
      await entry.click();
      const account = page.getByRole("region", { name: "账号设置", exact: true });
      await expect(account).toBeVisible();
      await expect(page.getByRole("dialog")).toHaveCount(0);
      const changedPassword = "changed-" + password;
      await account.getByLabel("当前密码", { exact: true }).fill("incorrect-password");
      await account.getByLabel("新密码", { exact: true }).fill(changedPassword);
      await account.getByLabel("确认新密码", { exact: true }).fill(changedPassword);
      await account.getByRole("button", { name: "保存", exact: true }).click();
      await expect(account.getByRole("alert")).toHaveText("当前密码不正确。");
      await expect(account.locator(".xcss-request-id")).toHaveCount(0);
      for (const label of ["当前密码", "新密码", "确认新密码"]) await expect(account.getByLabel(label, { exact: true })).toHaveValue("");
      assert.equal(JSON.stringify(await context.cookies()), JSON.stringify(originalCookies), "Wrong current password does not replace the session cookie");
      assert.equal((await context.request.get(base + "/api/v1/auth/session")).status(), 200);
      assert.equal((await other.request.get(base + "/api/v1/auth/session")).status(), 200);
      const session = await (await context.request.get(base + "/api/v1/auth/session")).json();
      const obsolete = await context.request.post(base + "/api/v1/monitoring/account/verify", {
        headers: { ...headers, "x-csrf-token": session.csrf_token }, data: { username: "admin", password },
      });
      assert.ok([404, 405].includes(obsolete.status()), "The two-step preview verification route has been removed");
      await account.getByLabel("当前密码", { exact: true }).fill(password);
      await account.getByLabel("新密码", { exact: true }).fill(changedPassword);
      await account.getByLabel("确认新密码", { exact: true }).fill(changedPassword);
      await account.getByRole("button", { name: "保存", exact: true }).click();
      await expect(page.getByRole("button", { name: "登录", exact: true })).toBeVisible();
      assert.equal((await other.request.get(base + "/api/v1/auth/session")).status(), 401);
      assert.equal((await other.request.post(base + "/api/v1/auth/login", { headers, data: { username: "admin", password } })).status(), 401);
      await page.getByLabel("用户名", { exact: true }).fill("admin");
      await page.getByLabel("密码", { exact: true }).fill(changedPassword);
      await page.getByRole("button", { name: "登录", exact: true }).click();
      await expect(entry).toBeVisible(); await entry.click();
      await account.getByLabel("用户名", { exact: true }).fill("renamed-admin");
      await account.getByLabel("当前密码", { exact: true }).fill(changedPassword);
      await account.getByRole("button", { name: "保存", exact: true }).click();
      await expect(page.getByRole("button", { name: "登录", exact: true })).toBeVisible();
      await page.getByLabel("用户名", { exact: true }).fill("renamed-admin");
      await page.getByLabel("密码", { exact: true }).fill(changedPassword);
      await page.getByRole("button", { name: "登录", exact: true }).click();
      await expect(entry).toBeVisible(); await entry.click();
      await expect(account.getByLabel("用户名", { exact: true })).toHaveValue("renamed-admin");
      assert.deepEqual(errors, []);
      console.log(`${engine.name()}: real Host ordinary account page; wrong password causes no mutation/session loss; save changes password and revokes old sessions; rename keeps password; obsolete verification route removed`);
    } finally { await context.close(); await other.close(); await browser.close(); }
  });
}
