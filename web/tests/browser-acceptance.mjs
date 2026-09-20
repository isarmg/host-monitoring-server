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
function aggregate(value) { return { count: value === null ? 0 : 1, min: value, max: value, avg: value }; }
function bucket(start, cpu, memory) {
  const absent = aggregate(null);
  return { start, end: new Date(Date.parse(start) + 5_000).toISOString(), sample_count: 1,
    cpu_usage_percent: aggregate(cpu), memory_usage_percent: aggregate(memory),
    network_received_bytes_per_second: absent, network_transmitted_bytes_per_second: absent,
    disk_read_bytes_per_second: absent, disk_written_bytes_per_second: absent,
    max_temperature_celsius: absent, gpu_utilization_percent: absent, gpu_memory_usage_percent: absent };
}
function latestReport() {
  return { schema_version: 1, report_id: "038f1f4b-7a5d-7b5f-8d31-000000000050", collected_at: "2026-09-04T00:00:00Z", host: { id: host(50).id, os: "linux", os_version: null, kernel_version: null, arch: "x86_64", client_version: "0.8.1" }, interval_seconds: 5,
    system: { uptime_seconds: 90061, cpu: { usage_percent: 12.5, logical_count: 8, physical_count: 4, per_core_percent: [10, 15] }, memory: { total_bytes: 17179869184, used_bytes: 8589934592, available_bytes: 8589934592, swap_total_bytes: 0, swap_used_bytes: 0 },
      networks: [{ name: "eth0", received_bytes_total: 1024, transmitted_bytes_total: 2048, received_bytes_per_second: 128, transmitted_bytes_per_second: 256, packets_received_total: 10, packets_transmitted_total: 20, receive_errors_total: 0, transmit_errors_total: 0 }],
      disks: [{ name: "nvme0n1", mount_point: "/", file_system: "ext4", total_bytes: 1000000000, available_bytes: 500000000, read_bytes_total: 4096, written_bytes_total: 8192, read_bytes_per_second: 512, written_bytes_per_second: 1024, is_read_only: false }],
      temperatures: [{ id: "cpu", label: "CPU Package", celsius: 48.5, max_celsius: 90, critical_celsius: 100, source: "sysfs" }],
      gpus: [{ id: "gpu0", vendor: "NVIDIA", name: "RTX", utilization_percent: 35, memory_total_bytes: 8589934592, memory_used_bytes: 4294967296, temperature_celsius: 55, power_watts: 120, core_clock_mhz: 1500, memory_clock_mhz: 7000, pcie_rx_bytes_per_second: 256, pcie_tx_bytes_per_second: 128, source: "nvml" }] },
    capabilities: [{ name: "system.cpu", available: true, source: "sysinfo", error_kind: null, message: null }], client: { spool_pending_batches: 0, collector_errors: 0 } };
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
      let detailActive = 0, detailRequests = 0, maximumDetailActive = 0, historyRequests = 0, failNextHistory = false, deleted = false;
      page.on("pageerror", error => errors.push(error.message));
      await page.route("**/api/v2/**", async route => {
        const request = route.request();
        const url = new URL(request.url());
        const offset = Number(url.searchParams.get("offset") ?? "0");
        const isHosts = url.pathname.endsWith("/monitoring/hosts");
        if (isHosts) requested.push(offset);
        const detailMatch = /\/monitoring\/hosts\/([0-9a-f-]+)$/.exec(url.pathname);
        const historyMatch = /\/monitoring\/hosts\/([0-9a-f-]+)\/history$/.exec(url.pathname);
        let body;
        if (isHosts) {
          const hosts = Array.from({ length: 51 }, (_, index) => host(index)).filter(value => !deleted || value.id !== host(50).id);
          body = { hosts, statistics: { total: { total: hosts.length, online: 0 }, windows: { total: 0, online: 0 }, linux: { total: hosts.length, online: 0 }, macos: { total: 0, online: 0 } }, total: hosts.length, limit: 1000, offset };
        } else if (url.pathname.endsWith("/client-instances")) {
          instanceRequested.push(offset);
          const indexes = Array.from({ length: Math.min(50, 51 - offset) }, (_, index) => offset + index).filter(index => !deleted || index !== 50);
          body = { instances: indexes.map(instance), hosts: indexes.map(host), total: deleted ? 50 : 51, limit: 50, offset };
        } else if (historyMatch) {
          historyRequests++;
          if (failNextHistory) {
            failNextHistory = false;
            return route.fulfill({ status: 503, json: { code: "service_unavailable", message: "SECRET history", retryable: true, request_id: "history-failure-123" } });
          }
          await new Promise(resolve => setTimeout(resolve, 2_500));
          body = { host_id: historyMatch[1], requested_from: "2026-09-03T23:00:00Z", requested_to: "2026-09-04T00:00:00Z",
            actual_from: "2026-09-03T23:00:00Z", actual_to: "2026-09-03T23:59:00Z", step_seconds: 5, source: "raw", points: [
              bucket("2026-09-03T23:00:00Z", 10, 20), bucket("2026-09-03T23:01:00Z", 20, null), bucket("2026-09-03T23:59:00Z", 30, 40),
            ] };
        } else if (detailMatch) {
          detailRequests++; detailActive++; maximumDetailActive = Math.max(maximumDetailActive, detailActive);
          await new Promise(resolve => setTimeout(resolve, 250));
          detailActive--;
          body = { host: host(50), latest: latestReport() };
        } else if (url.pathname.endsWith(`/monitoring/managed-instances/${host(50).id}`) && request.method() === "DELETE") {
          deleted = true;
          return route.fulfill({ status: 204 });
        } else {
          body = session;
        }
        return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(body) });
      });
      await page.goto(`http://127.0.0.1:${address.port}`);
      await page.getByRole("link", { name: "选择实例 Host-0", exact: true }).waitFor();
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
      assert.deepEqual(await table.getByRole("columnheader").allTextContents(), ["名称", "配对状态", "在线状态", "系统 / 架构", "授权码", "操作", "删除"]);
      const cells = table.locator("tbody tr").first().locator("td");
      assert.deepEqual((await cells.allTextContents()).slice(0, 3), ["已配对", "在线", "linux / x86_64"]);
      await expect(cells.nth(3).locator("code")).toHaveText("uci_00000000000000000000000000000000");
      await expect(cells.nth(3).getByRole("button", { name: "复制", exact: true })).toHaveCount(0);
      assert.equal(await table.locator("tbody tr").first().evaluate(row => getComputedStyle(row).display), "table-row");
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
      assert.deepEqual((await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze()).violations, []);
      assert.equal(await page.getByRole("complementary").count(), 0);
      assert.ok((await page.locator("#hosts > h1").boundingBox()).height <= 1);
      await page.getByRole("button", { name: "下一页", exact: true }).click();
      await expect(table.locator("tbody tr")).toHaveCount(1);
      await page.getByRole("link", { name: "选择实例 Host-50", exact: true }).click();
      await expect.poll(() => detailActive).toBe(1);
      await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
      await expect(page.getByRole("button", { name: "详细信息", exact: true })).toHaveAttribute("aria-pressed", "true");
      assert.deepEqual([...new Set(requested)], [0]);
      assert.deepEqual([...new Set(instanceRequested)], [0, 50]);
      await page.getByRole("heading", { name: "最新设备信息", exact: true }).waitFor();
      await page.getByText("16.0 GiB", { exact: true }).waitFor();
      await expect(page.locator("pre")).toHaveCount(0);
      await page.getByRole("heading", { name: "历史趋势", exact: true }).waitFor();
      await page.getByText("页面最近更新", { exact: true }).waitFor();
      await expect.poll(() => detailRequests).toBeGreaterThanOrEqual(2);
      assert.equal(maximumDetailActive, 1);
      await expect.poll(() => historyRequests).toBeGreaterThanOrEqual(1);
      const cpuPoints = page.getByRole("img", { name: "CPU 历史图" }).locator("polyline").first();
      await expect(cpuPoints).toHaveAttribute("points", /0,90 1\.6666.*?,80 98\.3333.*?,70/);
      await page.getByRole("button", { name: "暂停自动更新（2 秒）", exact: true }).click();
      const beforeManualRefresh = detailRequests;
      await page.getByRole("group", { name: "全局操作" }).getByRole("button", { name: "刷新", exact: true }).click();
      await expect.poll(() => detailRequests).toBeGreaterThan(beforeManualRefresh);
      failNextHistory = true;
      await page.getByRole("button", { name: "24h", exact: true }).click();
      await expect(page.getByRole("alert")).toContainText("history-failure-123");
      await expect(page.locator("body")).not.toContainText("SECRET history");
      await page.getByRole("alert").getByRole("button", { name: "重试", exact: true }).click();
      await expect(page.getByRole("img", { name: "CPU 历史图" })).toBeVisible();
      for (const theme of ["light", "dark"]) {
        if (await page.locator("html").getAttribute("data-theme") !== theme) await page.getByRole("button", { name: /切换到.*模式/ }).click();
        const result = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
        assert.deepEqual(result.violations, []);
        assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
      }
      const selectedId = host(50).id;
      await checkWebLanguage(page, {"routes":[["instances","Instance list"],[`details/${selectedId}`,"Details"],[`logs/${selectedId}`,"Logs"]],"names":["验收主机","测试主机"]});
      await page.getByRole("button", { name: "删除实例", exact: true }).click();
      await page.getByRole("dialog", { name: "删除监控实例", exact: true }).getByRole("button", { name: "确认", exact: true }).click();
      await expect.poll(() => new URL(page.url()).hash).toBe("#instances");
      await expect(page.getByRole("link", { name: "选择实例 Host-50", exact: true })).toHaveCount(0);
      await checkHeaderLogout(page, session.csrf_token);
      assert.deepEqual(errors, []);
      console.log(`${engine.name()}: current Host build, pagination, full details and mobile light/dark WCAG AA passed`);
      await context.close();
    } finally { await browser.close(); }
  }
} finally { await new Promise((resolve, reject) => server.httpServer.close(error => error ? reject(error) : resolve())); }
