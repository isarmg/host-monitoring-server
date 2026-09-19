import { checkWebLanguage } from "./language.mjs";
import { checkHeaderActions, checkHeaderLogout } from "./header-actions.mjs";
import assert from "node:assert/strict";
import { chromium, firefox, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { preview } from "vite";

const session = { authenticated: true, user_id: "A".repeat(43), username: "admin", role: "admin", csrf_token: "A".repeat(43) };
function host(index) {
  return {
    id: "018f1f4b-7a5d-7b5f-8d31-" + String(index).padStart(12, "0"), name: "Host-" + index,
    os: "linux", os_version: null, kernel_version: null, arch: "x86_64", client_version: "0.8.1",
    registered_at: "2026-09-04T00:00:00Z", last_seen_at: "2026-09-04T00:00:00Z", latest_collected_at: null,
    status: "online", capabilities: [], cpu_usage_percent: null, memory_usage_percent: 25,
    network_received_bytes_per_second: null, network_transmitted_bytes_per_second: null,
    disk_read_bytes_per_second: null, disk_written_bytes_per_second: null,
    max_temperature_celsius: null, gpu_utilization_percent: null, gpu_memory_usage_percent: null,
  };
}
function instance(index) {
  return {
    request_id: "028f1f4b-7a5d-7b5f-8d31-" + String(index).padStart(12, "0"),
    instance_id: host(index).id,
    display_name: "Host-" + index,
    status: "active",
    created_at: "2026-09-04T00:00:00Z",
    authorization_code: "uci_" + String(index).padStart(32, "0"),
  };
}
const server = await preview({ preview: { host: "127.0.0.1", port: 0, strictPort: true } });
const address = server.httpServer.address();
assert.ok(address && typeof address === "object");
try {
  for (const engine of [chromium, firefox]) {
    const browser = await engine.launch();
    try {
      const context = await browser.newContext({ locale: "zh-CN",  viewport: { width: 360, height: 740 } });
      const page = await context.newPage();
      const errors = [];
      const requested = [];
      const instanceRequested = [];
      page.on("pageerror", error => errors.push(error.message));
      await page.route("**/api/v2/**", route => {
        const url = new URL(route.request().url());
        const offset = Number(url.searchParams.get("offset") ?? "0");
        const isHosts = url.pathname.endsWith("/monitoring/hosts");
        if (isHosts) requested.push(offset);
        const detailMatch = /\/monitoring\/hosts\/([0-9a-f-]+)$/.exec(url.pathname);
        const historyMatch = /\/monitoring\/hosts\/([0-9a-f-]+)\/history$/.exec(url.pathname);
        let body;
        if (isHosts) {
          body = { hosts: Array.from({ length: 51 }, (_, index) => host(index)), total: 51, limit: 1000, offset };
        } else if (url.pathname.endsWith("/client-instances")) {
          instanceRequested.push(offset);
          const indexes = Array.from({ length: Math.min(50, 51 - offset) }, (_, index) => offset + index);
          body = { instances: indexes.map(instance), hosts: indexes.map(host), total: 51, limit: 50, offset };
        } else if (historyMatch) {
          body = { host_id: historyMatch[1], requested_from: "2026-09-03T23:00:00Z", requested_to: "2026-09-04T00:00:00Z",
            actual_from: "2026-09-03T23:00:00Z", actual_to: "2026-09-04T00:00:00Z", step_seconds: 5, source: "raw", points: [] };
        } else if (detailMatch) {
          body = { host: host(50), latest: null };
        } else {
          body = session;
        }
        return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(body) });
      });
      await page.goto(`http://127.0.0.1:${address.port}`);
      await page.getByRole("button", { name: "选择实例 Host-0", exact: true }).waitFor();
      await checkHeaderActions(page, "/monitoring/hosts");
      await page.locator("#statistics-heading").waitFor();
      const spacing = await page.evaluate(() => {
        const header = document.querySelector(".sarmg-page-header");
        const first = document.querySelector("#statistics-heading");
        const firstSection = first?.closest("section");
        const second = document.querySelector("#instances-heading");
        if (!header || !first || !firstSection || !second) throw new Error("Host spacing fixture is incomplete");
        return {
          menuToFirst: first.getBoundingClientRect().top - header.getBoundingClientRect().bottom,
          sectionToSubheading: second.getBoundingClientRect().top - firstSection.getBoundingClientRect().bottom,
        };
      });
      assert.ok(Math.abs(spacing.menuToFirst - 16) < 2, JSON.stringify(spacing));
      assert.ok(Math.abs(spacing.sectionToSubheading - 16) < 2, JSON.stringify(spacing));
      await expect(page.getByRole("button", { name: "实例列表", exact: true })).toHaveAttribute("aria-pressed", "true");
      assert.equal(await page.getByRole("button", { name: "Diagnostics", exact: true }).count(), 0);
      const table = page.getByRole("table", { name: "实例列表" });
      await expect(table.locator("tbody tr")).toHaveCount(50);
      assert.deepEqual(await table.getByRole("columnheader").allTextContents(), ["名称", "配对状态", "在线状态", "系统 / 架构", "授权码", "操作"]);
      const cells = table.locator("tbody tr").first().locator("td");
      assert.deepEqual((await cells.allTextContents()).slice(0, 3), ["已配对", "在线", "linux / x86_64"]);
      await expect(cells.nth(3).locator("code")).toHaveText("uci_00000000000000000000000000000000");
      await expect(cells.nth(3).getByRole("button", { name: "复制", exact: true })).toBeVisible();
      assert.equal(await table.locator("tbody tr").first().evaluate(row => getComputedStyle(row).display), "table-row");
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
      assert.deepEqual((await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze()).violations, []);
      assert.equal(await page.getByRole("complementary").count(), 0);
      assert.ok((await page.locator("#hosts > h1").boundingBox()).height <= 1);
      await page.getByRole("button", { name: "下一页", exact: true }).click();
      await expect(table.locator("tbody tr")).toHaveCount(1);
      await page.getByRole("button", { name: "选择实例 Host-50", exact: true }).click();
      await expect(page.getByRole("button", { name: "详细信息", exact: true })).toHaveAttribute("aria-pressed", "true");
      assert.deepEqual([...new Set(requested)], [0]);
      assert.deepEqual([...new Set(instanceRequested)], [0, 50]);
      await page.getByRole("heading", { name: "最新设备信息", exact: true }).waitFor();
      await page.getByText("等待首次上报", { exact: true }).waitFor();
      await page.getByRole("heading", { name: "历史趋势", exact: true }).waitFor();
      await page.getByText("页面最近更新", { exact: true }).waitFor();
      for (const theme of ["light", "dark"]) {
        if (await page.locator("html").getAttribute("data-theme") !== theme) await page.getByRole("button", { name: /切换到.*模式/ }).click();
        const result = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
        assert.deepEqual(result.violations, []);
        assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
      }
      const selectedId = host(50).id;
      await checkWebLanguage(page, {"routes":[["instances","Instance list"],[`details/${selectedId}`,"Details"],[`logs/${selectedId}`,"Logs"]],"names":["验收主机","测试主机"]});
      await checkHeaderLogout(page, session.csrf_token);
      assert.deepEqual(errors, []);
      console.log(`${engine.name()}: current Host build, pagination, full details and mobile light/dark WCAG AA passed`);
      await context.close();
    } finally { await browser.close(); }
  }
} finally { await new Promise((resolve, reject) => server.httpServer.close(error => error ? reject(error) : resolve())); }
