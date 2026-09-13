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
    os: "linux", os_version: null, kernel_version: null, arch: "x86_64", client_version: "0.8.0",
    registered_at: "2026-09-04T00:00:00Z", last_seen_at: "2026-09-04T00:00:00Z", latest_collected_at: null,
    status: "online", capabilities: [], cpu_usage_percent: null, memory_usage_percent: 25,
    network_received_bytes_per_second: null, network_transmitted_bytes_per_second: null,
    disk_read_bytes_per_second: null, disk_written_bytes_per_second: null,
    max_temperature_celsius: null, gpu_utilization_percent: null, gpu_memory_usage_percent: null,
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
      page.on("pageerror", error => errors.push(error.message));
      await page.route("**/api/v2/**", route => {
        const url = new URL(route.request().url());
        const offset = Number(url.searchParams.get("offset") ?? "0");
        const isHosts = url.pathname.endsWith("/monitoring/hosts");
        if (isHosts) requested.push(offset);
        return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(isHosts
          ? { hosts: offset === 0 ? Array.from({ length: 50 }, (_, index) => host(index)) : [host(50)], total: 51, limit: 50, offset }
          : url.pathname.endsWith("/client-instances") ? [] : session) });
      });
      await page.goto(`http://127.0.0.1:${address.port}`);
      await page.getByRole("button", { name: "选择实例 Host-0", exact: true }).waitFor();
      await checkHeaderActions(page, "/monitoring/hosts");
      const spacing = await page.evaluate(() => {
        const header = document.querySelector(".sarmg-page-header");
        const first = document.querySelector("#instances-heading");
        const firstSection = first?.closest("section");
        const second = [...document.querySelectorAll("h2")].find(node => node.textContent === "实例总览");
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
      const table = page.getByRole("table", { name: "监控实例列表" });
      assert.equal(await table.locator("tbody tr").count(), 50);
      assert.deepEqual(await table.getByRole("columnheader").allTextContents(), ["实例名称", "状态", "系统 / 架构", "CPU 使用率", "内存使用率", "最近连接", "最近上报"]);
      const cells = table.locator("tbody tr").first().locator("td");
      assert.deepEqual((await cells.allTextContents()).slice(0, 4), ["在线", "linux / x86_64", "未上报", "25.0%"]);
      assert.equal(await cells.last().textContent(), "尚未上报");
      assert.equal(await table.locator("tbody tr").first().evaluate(row => getComputedStyle(row).display), "table-row");
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
      assert.deepEqual((await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze()).violations, []);
      assert.equal(await page.getByRole("complementary").count(), 0);
      assert.ok((await page.locator("#hosts > h1").boundingBox()).height <= 1);
      await page.getByRole("button", { name: "下一页" }).click();
      await page.getByRole("button", { name: "选择实例 Host-50", exact: true }).waitFor();
      assert.equal(await table.locator("tbody tr").count(), 1);
      await page.getByRole("button", { name: "选择实例 Host-50", exact: true }).click();
      await expect(page.getByRole("button", { name: "详细信息", exact: true })).toHaveAttribute("aria-pressed", "true");
      assert.ok(requested.includes(50));
      await page.getByText("完整采集信息").click();
      assert.equal(await page.getByText("client_version", { exact: true }).count(), 0);
      await page.getByText("注册时间", { exact: true }).waitFor();
      for (const theme of ["light", "dark"]) {
        if (await page.locator("html").getAttribute("data-theme") !== theme) await page.getByRole("button", { name: /切换到.*模式/ }).click();
        const result = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
        assert.deepEqual(result.violations, []);
        assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
      }
      await checkWebLanguage(page, {"routes":[["instances","Instance list"],["details","Details"],["logs","Logs"]],"names":["验收主机","测试主机"]});
      await checkHeaderLogout(page, session.csrf_token);
      assert.deepEqual(errors, []);
      console.log(`${engine.name()}: current Host build, pagination, full details and mobile light/dark WCAG AA passed`);
      await context.close();
    } finally { await browser.close(); }
  }
} finally { await new Promise((resolve, reject) => server.httpServer.close(error => error ? reject(error) : resolve())); }
