import assert from "node:assert/strict";
import { expect } from "@playwright/test";

export async function checkHeaderActions(page, listPath) {
  const header = page.getByRole("banner");
  const actions = header.getByRole("group", { name: "全局操作" });
  const buttons = actions.getByRole("button");
  await expect(buttons).toHaveCount(6);
  assert.deepEqual(await buttons.evaluateAll(nodes => nodes.map(node => node.getAttribute("aria-label"))),
    ["新建实例", "刷新", "切换为英文", await buttons.nth(3).getAttribute("aria-label"), "退出", "账号设置"]);
  assert.match(await buttons.nth(3).getAttribute("aria-label"), /^切换到.*模式$/);
  for (let index = 0; index < 6; index++) {
    const button = buttons.nth(index);
    assert.equal((await button.innerText()).trim(), "");
    await expect(button.locator('svg[aria-hidden="true"]')).toHaveCount(1);
    assert.equal(await button.getAttribute("title"), await button.getAttribute("aria-label"));
  }
  for (const name of ["新建实例", "刷新", "退出", "账号设置"]) {
    await expect(page.getByRole("button", { name, exact: true })).toHaveCount(1);
    await expect(page.getByRole("main").getByRole("button", { name, exact: true })).toHaveCount(0);
  }
  const previousViewport = page.viewportSize();
  for (const width of [320, 1280]) {
    await page.setViewportSize({ width, height: 800 });
    const rects = await buttons.evaluateAll(nodes => nodes.map(node => {
      const rect = node.getBoundingClientRect();
      return { x: rect.x, y: rect.y, width: rect.width, height: rect.height, right: rect.right };
    }));
    assert.ok(rects.every(rect => Math.abs(rect.y - rects[0].y) < 1 && rect.width >= 24 && rect.height >= 24));
    assert.ok(rects.slice(1).every((rect, i) => rect.x >= rects[i].right));
    assert.ok(Math.abs(rects[5].right - (width - 16)) < 2, "actions must align to the top-right header padding: " + JSON.stringify({width,rects}));
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  }
  await page.setViewportSize(previousViewport);
  await buttons.first().focus();
  for (let index = 1; index < 6; index++) {
    await page.keyboard.press("Tab");
    await expect(buttons.nth(index)).toBeFocused();
  }
  const refreshed = page.waitForRequest(request => new URL(request.url()).pathname.endsWith(listPath) && request.method() === "GET");
  await buttons.nth(1).click();
  await refreshed;
}

export async function checkHeaderLogout(page, csrfToken) {
  let loggedOut = false;
  await page.route("**/api/v2/auth/logout", route => {
    assert.equal(route.request().method(), "POST");
    assert.equal(route.request().headers()["x-csrf-token"], csrfToken);
    loggedOut = true;
    return route.fulfill({ status: 204 });
  });
  await page.getByRole("banner").getByRole("button", { name: "退出", exact: true }).click();
  await expect(page.getByRole("button", { name: "登录", exact: true })).toBeVisible();
  await expect.poll(() => loggedOut).toBe(true);
  await expect(page.getByRole("group", { name: "全局操作" })).toHaveCount(0);
}
