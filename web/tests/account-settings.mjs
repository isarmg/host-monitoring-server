import assert from "node:assert/strict";
import { chromium, firefox, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { preview } from "vite";

const token = "A".repeat(43), originalPassword = "correct-password", newPassword = "new-correct-password";
const requestId = "fad86f79a625d937c7e2ac6de06110d1";
const server = await preview({ preview: { host: "127.0.0.1", port: 0, strictPort: true } });
const address = server.httpServer.address();
assert.ok(address && typeof address === "object");

async function iconSizes(controls) {
  return Promise.all(["切换为英文", /切换到.*模式/].map(name => controls.getByRole("button", { name }).locator("svg").evaluate(node => {
    const { width, height } = node.getBoundingClientRect(); return { width, height };
  })));
}

async function entryAppearance(entry) {
  return entry.evaluate(node => {
    const style = getComputedStyle(node), svg = node.querySelector("svg"), rect = svg.getBoundingClientRect();
    return { color: style.color, background: style.backgroundColor, decoration: style.textDecorationLine, border: style.border,
      radius: style.borderRadius, width: rect.width, height: rect.height, image: svg.innerHTML };
  });
}

async function lightLogin(card) {
  const colors = await card.evaluate(node => ({ background: getComputedStyle(node).backgroundColor,
    text: [...node.querySelectorAll(".xcss-form-field > span,input,button")].map(element => getComputedStyle(element).color) }));
  assert.equal(colors.background, "rgb(242, 242, 242)");
  assert.ok(colors.text.every(color => color === "rgb(0, 0, 0)"));
}

try {
  for (const engine of [chromium, firefox]) {
    const browser = await engine.launch();
    try {
      for (const width of [360, 1280]) {
        const colorScheme = width === 360 ? "light" : "dark", opposite = colorScheme === "light" ? "dark" : "light";
        const context = await browser.newContext({ viewport: { width, height: 800 }, locale: "zh-CN", colorScheme });
        const page = await context.newPage(); page.setDefaultTimeout(30_000);
        const errors = [], fonts = [], attempts = [], saves = [];
        let authenticated = false, username = "admin", password = originalPassword, releaseSave;
        let delayNextSave = true, rejectNextSave = true;
        page.on("pageerror", error => errors.push(error.message));
        page.on("request", request => { if (new URL(request.url()).pathname.endsWith(".woff2")) fonts.push(request.url()); });
        await page.route("**/api/v1/**", async route => {
          const request = route.request(), path = new URL(request.url()).pathname;
          const session = { authenticated: true, user_id: token, username, role: "admin", csrf_token: token };
          const failure = (status, code) => route.fulfill({ status, headers: { "x-request-id": requestId },
            json: { code, message: "Private diagnostic", retryable: false, request_id: requestId } });
          if (path.endsWith("/auth/session")) return authenticated ? route.fulfill({ json: session }) : failure(401, "auth.session_required");
          if (path.endsWith("/auth/login")) {
            assert.deepEqual(request.postDataJSON(), { username, password }); authenticated = true;
            return route.fulfill({ json: session });
          }
          if (request.method() !== "GET") assert.equal(request.headers()["x-csrf-token"], token);
          if (path.endsWith("/platform/administrators/self")) {
            const input = request.postDataJSON(); attempts.push(input);
            if (input.current_password !== password) return failure(403, "admin.current_password_invalid");
            if (input.username === "taken-admin") return failure(409, "admin.conflict");
            if (delayNextSave) { delayNextSave = false; await new Promise(resolve => { releaseSave = resolve; }); }
            if (rejectNextSave) { rejectNextSave = false; return failure(503, "platform.unavailable"); }
            saves.push(input); username = input.username; password = input.new_password ?? password; authenticated = false;
            return route.fulfill({ status: 204 });
          }
          if (path.endsWith("/monitoring/hosts")) return route.fulfill({ json: { hosts: [], next_cursor: null, previous_cursor: null,
            statistics: Object.fromEntries(["total", "windows", "linux", "macos"].map(key => [key, { total: 0, online: 0 }])) } });
          if (path.endsWith("/client-instances")) return route.fulfill({ json: { instances: [], hosts: [], next_cursor: null, previous_cursor: null } });
          throw new Error("Unexpected API path: " + path);
        });
        await page.goto(`http://127.0.0.1:${address.port}/`);
        await expect(page.getByRole("button", { name: "登录", exact: true })).toBeVisible();
        await expect(page.locator("html")).toHaveAttribute("data-theme", colorScheme);
        const loginControls = page.getByRole("group", { name: "显示设置", exact: true });
        const loginIcons = await iconSizes(loginControls);
        if (colorScheme === "light") await lightLogin(page.locator(".xcss-auth-card"));
        await loginControls.getByRole("button", { name: /切换到.*模式/ }).click();
        await page.reload();
        await expect(page.getByRole("button", { name: "登录", exact: true })).toBeVisible();
        await expect(page.locator("html")).toHaveAttribute("data-theme", opposite);
        if (opposite === "light") await lightLogin(page.locator(".xcss-auth-card"));
        await page.getByLabel("用户名", { exact: true }).fill(username);
        await page.getByLabel("密码", { exact: true }).fill(password);
        await page.getByRole("button", { name: "登录", exact: true }).click();
        const header = page.getByRole("banner"), entry = header.getByRole("button", { name: "账号设置", exact: true });
        await expect(entry).toBeVisible();
        assert.deepEqual(await iconSizes(header), loginIcons);
        await header.getByRole("button", { name: /切换到.*模式/ }).click();
        await expect(page.locator("html")).toHaveAttribute("data-theme", colorScheme);
        const content = page.locator(".xcss-table-scroll:has(.xcss-statistics-table)");
        await expect(content).toBeVisible();
        const contentColor = await content.evaluate(node => getComputedStyle(node).backgroundColor);
        const before = await entryAppearance(entry);
        const account = page.getByRole("region", { name: "账号设置", exact: true });
        async function open() {
          await entry.click(); await page.mouse.move(0, 0);
          await expect(account).toBeVisible();
          await expect.poll(() => new URL(page.url()).hash).toBe("#account");
          await expect(page.getByRole("dialog")).toHaveCount(0);
          await expect(page.locator("dialog:modal")).toHaveCount(0);
          assert.deepEqual(await entryAppearance(entry), before);
          assert.equal(await entry.getAttribute("aria-pressed"), null);
          assert.equal(await entry.getAttribute("aria-current"), null);
          assert.equal(await account.locator(".xcss-content-panel").evaluate(node => getComputedStyle(node).backgroundColor), contentColor);
          await expect(page.locator('.xcss-header-navigation [aria-pressed="true"]')).toHaveCount(0);
        }
        await open();
        assert.deepEqual(await account.locator(".xcss-form-field > span").allTextContents(), ["用户名", "当前密码", "新密码", "确认新密码"]);
        assert.deepEqual(await account.getByRole("button").allTextContents(), ["保存"]);
        await expect(account.getByLabel("用户名", { exact: true })).toHaveValue("admin");
        await account.getByLabel("用户名", { exact: true }).fill("unsaved-admin");
        for (const label of ["当前密码", "新密码", "确认新密码"]) await account.getByLabel(label, { exact: true }).fill("discard-this-secret");
        await page.getByRole("button", { name: "实例列表", exact: true }).click();
        await expect(account).toHaveCount(0);
        await open();
        await expect(account.getByLabel("用户名", { exact: true })).toHaveValue("admin");
        for (const label of ["当前密码", "新密码", "确认新密码"]) await expect(account.getByLabel(label, { exact: true })).toHaveValue("");
        await page.goBack(); await expect(account).toHaveCount(0);
        await page.goForward(); await expect(account).toBeVisible();
        await page.reload(); await expect(account).toBeVisible();
        await expect(page.locator("html")).toHaveAttribute("data-theme", colorScheme);
        const fontCount = fonts.length, storage = await page.evaluate(() => ({ local: { ...localStorage }, session: { ...sessionStorage } }));
        const axe = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
        assert.deepEqual(axe.violations, []);
        await account.getByRole("button", { name: "保存", exact: true }).click();
        await expect(account.getByRole("alert")).toHaveText("请输入当前密码。");
        await expect(account.getByLabel("当前密码", { exact: true })).toBeFocused();
        assert.equal(attempts.length, 0);
        async function fill(current = originalPassword, next = newPassword) {
          await account.getByLabel("当前密码", { exact: true }).fill(current);
          await account.getByLabel("新密码", { exact: true }).fill(next);
          await account.getByLabel("确认新密码", { exact: true }).fill(next);
        }
        await fill(originalPassword, "short");
        await account.getByRole("button", { name: "保存", exact: true }).click();
        await expect(account.getByRole("alert")).toHaveText("新密码长度不符合要求。");
        await fill(); await account.getByLabel("确认新密码", { exact: true }).fill("different-password");
        await account.getByRole("button", { name: "保存", exact: true }).click();
        await expect(account.getByRole("alert")).toHaveText("两次输入的新密码不一致。");
        await expect(account.getByLabel("确认新密码", { exact: true })).toBeFocused();
        assert.equal(attempts.length, 0);
        await fill("incorrect-password");
        await account.getByRole("button", { name: "保存", exact: true }).click();
        await expect(account.getByRole("alert")).toHaveText("当前密码不正确。");
        await expect(account.locator(".xcss-request-id")).toHaveCount(0);
        await expect(account).not.toContainText(requestId);
        for (const label of ["当前密码", "新密码", "确认新密码"]) await expect(account.getByLabel(label, { exact: true })).toHaveValue("");
        assert.equal(saves.length, 0); await expect(account).toBeVisible();
        await account.getByLabel("用户名", { exact: true }).fill("taken-admin"); await fill();
        await account.getByRole("button", { name: "保存", exact: true }).click();
        await expect(account.getByRole("alert")).toHaveText("用户名已被使用。");
        await account.getByLabel("用户名", { exact: true }).fill("admin"); await fill();
        await account.getByRole("button", { name: "保存", exact: true }).click();
        await expect.poll(() => typeof releaseSave).toBe("function");
        await expect(account.getByRole("button", { name: "保存中…", exact: true })).toBeDisabled();
        const count = attempts.length;
        await account.locator("form").evaluate(form => form.requestSubmit());
        assert.equal(attempts.length, count);
        releaseSave();
        await expect(account.getByRole("alert")).toHaveText("账号未能更新，请检查输入并重试。");
        await expect(account.locator(".xcss-request-id")).toHaveCount(0);
        for (const label of ["当前密码", "新密码", "确认新密码"]) await expect(account.getByLabel(label, { exact: true })).toHaveValue("");
        await fill(); await account.getByRole("button", { name: "保存", exact: true }).click();
        await expect(page.getByRole("button", { name: "登录", exact: true })).toBeVisible();
        assert.deepEqual(saves, [{ username: "admin", current_password: originalPassword, new_password: newPassword }]);
        await expect(page.locator("html")).toHaveAttribute("data-theme", colorScheme);
        await page.getByLabel("用户名", { exact: true }).fill("admin");
        await page.getByLabel("密码", { exact: true }).fill(newPassword);
        await page.getByRole("button", { name: "登录", exact: true }).click();
        await open(); await account.getByLabel("用户名", { exact: true }).fill("renamed-admin");
        await account.getByLabel("当前密码", { exact: true }).fill(newPassword);
        await account.getByRole("button", { name: "保存", exact: true }).click();
        await expect(page.getByRole("button", { name: "登录", exact: true })).toBeVisible();
        assert.deepEqual(saves.at(-1), { username: "renamed-admin", current_password: newPassword });
        assert.equal(fonts.length, fontCount);
        assert.deepEqual(await page.evaluate(() => ({ local: { ...localStorage }, session: { ...sessionStorage } })), storage);
        assert.deepEqual(errors, []);
        assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
        console.log(`${engine.name()} ${width}px ${colorScheme}: ordinary account route/back/forward/reload, unchanged person icon without underline, atomic save validation/errors/single submit, password/rename, login palette/icon size/theme persistence, no font reload and WCAG passed`);
        await context.close();
      }
    } finally { await browser.close(); }
  }
} finally { await new Promise((resolve, reject) => server.httpServer.close(error => error ? reject(error) : resolve())); }
