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
    max_temperature_celsius: null, gpu_utilization_percent: null, gpu_memory_usage_percent: null, cpu_frequency_mhz: null, gpu_power_watts: null, gpu_core_clock_mhz: null, max_fan_rpm: null, max_disk_temperature_celsius: null, max_disk_percentage_used: null,
  };
}
function instance(index) {
  return {
    request_id: "028f1f4b-7a5d-7b5f-8d31-" + String(index).padStart(12, "0"),
    instance_id: host(index).id,
    display_name: "Host-" + index,
    status: "active",
    created_at: "2026-09-04T00:00:00Z",
    authorization_code: String(index).padStart(36, "0"),
  };
}
function aggregate(value) { return { count: value === null ? 0 : 1, min: value, max: value, avg: value }; }
function bucket(start, cpu, memory) {
  const absent = aggregate(null);
  return { start, end: new Date(Date.parse(start) + 5_000).toISOString(), sample_count: 1,
    cpu_usage_percent: aggregate(cpu), memory_usage_percent: aggregate(memory),
    network_received_bytes_per_second: absent, network_transmitted_bytes_per_second: absent,
    disk_read_bytes_per_second: absent, disk_written_bytes_per_second: absent,
    max_temperature_celsius: absent, gpu_utilization_percent: absent, gpu_memory_usage_percent: absent, cpu_frequency_mhz: absent, gpu_power_watts: absent, gpu_core_clock_mhz: absent, max_fan_rpm: absent, max_disk_temperature_celsius: absent, max_disk_percentage_used: absent };
}
function latestReport(hostId = host(50).id, collectedAt = "2026-09-04T00:00:00Z") {
  return { schema_version: 3, report_id: collectedAt.endsWith("00:14:00Z") ? "038f1f4b-7a5d-7b5f-8d31-000000000051" : "038f1f4b-7a5d-7b5f-8d31-000000000050", collected_at: collectedAt, host: { id: hostId, os: "linux", os_version: null, kernel_version: null, arch: "x86_64", client_version: "0.8.1" }, interval_seconds: 5,
    system: { hardware: { collected_at: "2026-09-04T00:00:00Z", cpu: {model:"Modern CPU",frequency_mhz:4200,per_core_frequency_mhz:[4200,null],load_average:[0.1,0.2,0.3]},
      networks: [{name:"eth0",ip_addresses:["192.0.2.1/24"],link_speed_mbps:2500}, {name:"aux0",ip_addresses:["198.51.100.1/24"],link_speed_mbps:1000}],
      physical_networks: [{id:"pci-0000:03:00.0",name:"Intel I225-V",interface_name:"eth0",mac_address:"02:00:00:00:00:01",link_speed_mbps:2500,source:"linux-sysfs-net-device"}],
      sensors: [{id:"fan1",label:"CPU Fan",kind:"fan_rpm",value:1200,source:"linux-hwmon"}],
      disk_health: [{device:"/dev/nvme0",model:"NVMe SSD",healthy:false,percentage_used:105,media_errors:"9007199254740993",collected_at:"2026-09-04T00:00:00Z",source:"smartctl-json"}] }, uptime_seconds: 90061, cpu: { usage_percent: 12.5, logical_count: 8, physical_count: 4, per_core_percent: [10, 15] }, memory: { total_bytes: 17179869184, used_bytes: 8589934592, available_bytes: 8589934592, swap_total_bytes: 0, swap_used_bytes: 0 },
      networks: [{ name: "eth0", received_bytes_total: 1024, transmitted_bytes_total: 2048, received_bytes_per_second: 128, transmitted_bytes_per_second: 256, packets_received_total: 10, packets_transmitted_total: 20, receive_errors_total: 0, transmit_errors_total: 0 }],
      disks: [{ name: "nvme0n1", mount_point: "/", file_system: "ext4", total_bytes: 1000000000, available_bytes: 500000000, read_bytes_total: 4096, written_bytes_total: 8192, read_bytes_per_second: 512, written_bytes_per_second: 1024, is_read_only: false }],
      temperatures: [{ id: "cpu", label: "CPU Package", celsius: 48.5, max_celsius: 90, critical_celsius: 100, source: "sysfs" }],
      gpus: [{ id: "gpu0", vendor: "NVIDIA", name: "RTX", utilization_percent: 35, memory_total_bytes: 8589934592, memory_used_bytes: 4294967296, temperature_celsius: 55, power_watts: 120, core_clock_mhz: 1500, memory_clock_mhz: 7000, pcie_rx_bytes_per_second: 256, pcie_tx_bytes_per_second: 128, source: "nvml" },
      { id: "luid_00000000_0000a1bb", vendor: "amd", name: "AMD Radeon(TM) Vega 8 Graphics", utilization_percent: 40.3, memory_total_bytes: 1010 * 1024 * 1024, memory_used_bytes: 650 * 1024 * 1024, temperature_celsius: null, power_watts: null, core_clock_mhz: null, memory_clock_mhz: null, pcie_rx_bytes_per_second: null, pcie_tx_bytes_per_second: null, source: "windows-dxgi-pdh" },
      { id: "adlx_00000300", vendor: "amd", name: "AMD Radeon(TM) Vega 8 Graphics", utilization_percent: null, memory_total_bytes: null, memory_used_bytes: null, temperature_celsius: 88, power_watts: null, core_clock_mhz: null, memory_clock_mhz: null, pcie_rx_bytes_per_second: null, pcie_tx_bytes_per_second: null, source: "amd-adlx-no-luid" },
      { id: "luid_00000000_00013789", vendor: "amd", name: "AMD Radeon(TM) Vega 8 Graphics", utilization_percent: null, memory_total_bytes: 1010 * 1024 * 1024, memory_used_bytes: null, temperature_celsius: null, power_watts: null, core_clock_mhz: null, memory_clock_mhz: null, pcie_rx_bytes_per_second: null, pcie_tx_bytes_per_second: null, source: "windows-dxgi-pdh" }] },
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
      let detailActive = 0, detailRequests = 0, maximumDetailActive = 0, historyRequests = 0, failNextHistory = false, deleted = false, renamed = null;
      let reboundId = null, releaseReboundDetail = null, releaseFirstDetail = null, futureSample = false;
      const selectedHost = () => reboundId ? { ...host(50), id: reboundId, name: "Rebound Host", last_seen_at: "2026-09-04T00:10:00Z" } : host(50);
      page.on("pageerror", error => errors.push(error.message));
      await page.route("**/api/v2/**", async route => {
        const request = route.request();
        const url = new URL(request.url());
        const isHosts = url.pathname.endsWith("/monitoring/hosts");
        if (isHosts) requested.push(url.search);
        const detailMatch = /\/monitoring\/hosts\/([0-9a-f-]+)$/.exec(url.pathname);
        const historyMatch = /\/monitoring\/hosts\/([0-9a-f-]+)\/history$/.exec(url.pathname);
        const reportsMatch = /\/monitoring\/hosts\/([0-9a-f-]+)\/reports$/.exec(url.pathname);
        let body;
        if (isHosts) {
          const hosts = Array.from({ length: 51 }, (_, index) => index === 50 ? selectedHost() : host(index)).filter(value => !deleted || value.id !== selectedHost().id);
          body = { hosts, statistics: { total: { total: hosts.length, online: 0 }, windows: { total: 0, online: 0 }, linux: { total: hosts.length, online: 0 }, macos: { total: 0, online: 0 } } };
        } else if (url.pathname.endsWith("/client-instances")) {
          instanceRequested.push(url.search);
          const indexes = Array.from({ length: 51 }, (_, index) => index).filter(index => !deleted || index !== 50);
          body = { instances: indexes.map(index => ({ ...instance(index), ...(index === 50 ? { instance_id: selectedHost().id, ...(renamed ? { display_name: renamed } : {}) } : {}) })), hosts: indexes.map(index => index === 50 ? selectedHost() : host(index)) };
        } else if (url.pathname.endsWith(`/monitoring/client-instances/${instance(50).request_id}`) && request.method() === "PATCH") {
          assert.deepEqual(request.postDataJSON(), { display_name: "Renamed Host" });
          renamed = "Renamed Host";
          return route.fulfill({ status: 204 });
        } else if (url.pathname.endsWith("/monitoring/logs/calendar")) {
          body = { today: "2032-12-31" };
        } else if (reportsMatch) {
          const date = url.searchParams.get("date");
          assert.ok(["2032-12-31", "2032-12-30"].includes(date));
          body = { host_id: reportsMatch[1], date, reports: (date === "2032-12-31" ? [1, 2] : [3]).map(index => ({
            report_id: "038f1f4b-7a5d-7b5f-8d31-" + String(index).padStart(12, "0"),
            collected_at: `${date}T00:00:00Z`, received_at: `${date}T01:00:00Z`,
            collected_at_server: `${date} 08:00:00 +08:00`,
            received_at_server: `${date} 09:00:00 +08:00`,
          })) };
        } else if (historyMatch) {
          historyRequests++;
          if (failNextHistory) {
            failNextHistory = false;
            return route.fulfill({ status: 503, json: { code: "service_unavailable", message: "SECRET history", retryable: true, request_id: "history-failure-123" } });
          }
          await new Promise(resolve => setTimeout(resolve, 2_500));
          body = { host_id: historyMatch[1], requested_from: "2026-09-03T23:00:00Z", requested_to: "2026-09-04T00:00:00Z",
            actual_from: "2026-09-03T23:00:00Z", actual_to: "2026-09-03T23:59:00Z", step_seconds: 5, source: "raw", points: [
              bucket("2026-09-03T23:00:00Z", 10, 20), bucket("2026-09-03T23:00:05Z", 15, 25),
              bucket("2026-09-03T23:01:00Z", 20, null), bucket("2026-09-03T23:59:00Z", 30, 40),
            ] };
        } else if (detailMatch) {
          detailRequests++; detailActive++; maximumDetailActive = Math.max(maximumDetailActive, detailActive);
          if (detailRequests === 1) await new Promise(resolve => { releaseFirstDetail = resolve; });
          if (reboundId && detailMatch[1] === reboundId && releaseReboundDetail === null) await new Promise(resolve => { releaseReboundDetail = resolve; });
          await new Promise(resolve => setTimeout(resolve, 250));
          detailActive--;
          body = { host: selectedHost(), latest: latestReport(selectedHost().id, futureSample ? "2026-09-04T00:14:00Z" : "2026-09-04T00:00:00Z") };
        } else if (url.pathname.endsWith(`/monitoring/managed-instances/${selectedHost().id}`) && request.method() === "DELETE") {
          deleted = true;
          return route.fulfill({ status: 204 });
        } else {
          body = session;
        }
        return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(body) });
      });
      await page.goto(`http://127.0.0.1:${address.port}`);
      await page.getByRole("link", { name: "选择实例 Host-0", exact: true }).waitFor();
      const instanceNameStyle = await page.getByRole("link", { name: "选择实例 Host-0", exact: true }).evaluate(element => ({ color: getComputedStyle(element).color, parentColor: getComputedStyle(element.parentElement).color, decoration: getComputedStyle(element).textDecorationLine }));
      assert.equal(instanceNameStyle.color, instanceNameStyle.parentColor);
      assert.equal(instanceNameStyle.decoration, "none");
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
      await expect(table.locator("tbody tr")).toHaveCount(51);
      assert.deepEqual(await table.getByRole("columnheader").allTextContents(), ["实例名称", "配对状态", "在线状态", "操作系统/架构", "操作", "删除"]);
      const cells = table.locator("tbody tr").first().locator("td");
      assert.deepEqual((await cells.allTextContents()).slice(0, 3), ["已配对", "在线", "linux / x86_64"]);
      await expect(table).not.toContainText(host(0).id);
      await expect(table).not.toContainText("00000000000000000000000000000000");
      assert.equal(await table.locator("tbody tr").first().evaluate(row => getComputedStyle(row).display), "table-row");
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
      assert.deepEqual((await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze()).violations, []);
      assert.equal(await page.getByRole("complementary").count(), 0);
      assert.ok((await page.locator("#hosts > h1").boundingBox()).height <= 1);
      await page.getByRole("link", { name: "选择实例 Host-50", exact: true }).click();
      await expect.poll(() => detailActive).toBe(1);
      await expect.poll(() => typeof releaseFirstDetail).toBe("function");
      await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
      await expect(page.getByRole("button", { name: "详细信息", exact: true })).toHaveAttribute("aria-pressed", "true");
      const pairingDetails = page.getByRole("region", { name: "配对账户信息" });
      await expect(pairingDetails).toContainText(host(50).id);
      await expect(pairingDetails).toContainText(String(50).padStart(36, "0"));
      await page.getByLabel("实例名称", { exact: true }).fill("Renamed Host");
      await page.getByLabel("实例名称", { exact: true }).press("Enter");
      await expect(pairingDetails).toContainText("Renamed Host");
      releaseFirstDetail();
      await page.getByRole("heading", { name: "最新设备信息", exact: true }).waitFor();
      await expect(pairingDetails).toContainText("Renamed Host");
      assert.deepEqual([...new Set(requested)], [""]);
      assert.deepEqual([...new Set(instanceRequested)], [""]);
      const gpuSection = page.getByRole("heading", { name: "显卡", exact: true }).locator("..");
      await expect(gpuSection.locator(".host-device-card")).toHaveCount(2);
      const vega = gpuSection.locator(".host-device-card").filter({ has: page.getByRole("heading", { name: "AMD Radeon(TM) Vega 8 Graphics", exact: true }) });
      await expect(vega).toContainText("40.3%");
      await expect(vega).toContainText("650 MiB");
      await expect(vega).toContainText("88.0 ℃");
      await expect(vega).toContainText("luid_00000000_00013789");
      await expect(vega).toContainText("adlx_00000300");
      await page.getByText("16.0 GiB", { exact: true }).waitFor();
      await expect(page.locator("pre")).toHaveCount(0);
      await expect(page.getByRole("heading", {name:"硬件传感器",exact:true})).toBeVisible();
      await expect(page.getByText("1,200 RPM", {exact:true})).toBeVisible();
      await expect(page.getByText("105.0%", {exact:true})).toBeVisible();
      await expect(page.getByText("9007199254740993", {exact:true})).toBeVisible();
      await expect(page.getByText("192.0.2.1/24", {exact:true})).toBeVisible();
      const networkSection = page.getByRole("heading", { name: "网络接口", exact: true }).locator("..");
      await expect(networkSection.locator(".host-device-card")).toHaveCount(2);
      const eth0 = networkSection.locator(".host-device-card").filter({ has: page.getByRole("heading", { name: "eth0", exact: true }) });
      await expect(eth0).toContainText("192.0.2.1/24");
      await expect(eth0).toContainText("128 B/秒");
      await expect(networkSection.locator(".host-device-card").filter({ has: page.getByRole("heading", { name: "aux0", exact: true }) })).toContainText("198.51.100.1/24");
      const physicalNetwork = page.getByRole("heading", { name: "网络硬件", exact: true }).locator("..");
      await expect(physicalNetwork.locator(".host-device-card")).toHaveCount(1);
      await expect(physicalNetwork).toContainText("Intel I225-V");
      await expect(physicalNetwork).toContainText("pci-0000:03:00.0");
      await page.getByRole("heading", { name: "历史趋势", exact: true }).waitFor();
      await page.getByText("页面最近更新", { exact: true }).waitFor();
      await expect(page.getByText("服务端仍在收到上报，但当前展示的指标采集时间明显较早；客户端可能正在补传或已调整时钟。", { exact: true })).toHaveCount(0);
      await expect(page.getByText("指标采集时间明显晚于服务端接收时间；客户端时钟可能偏快，后续指标可能暂时不更新。", { exact: true })).toHaveCount(0);
      await expect.poll(() => detailRequests).toBeGreaterThanOrEqual(2);
      assert.equal(maximumDetailActive, 1);
      await expect.poll(() => historyRequests).toBeGreaterThanOrEqual(1);
      const cpuChart = page.getByRole("img", { name: "CPU 历史图" });
      await expect(cpuChart.locator("polyline")).toHaveCount(1);
      await expect(cpuChart.locator("polyline")).toHaveAttribute("points", /^0,90 0\.1388.*?,85$/);
      await expect(cpuChart.locator("circle")).toHaveCount(2);
      assert.deepEqual(await cpuChart.locator("circle").evaluateAll(elements => elements.map(element => Number(element.getAttribute("cy")))), [80, 70]);
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
      const selectedId = instance(50).request_id;
      await page.getByRole("button", { name: "日志", exact: true }).click();
      const logDate = page.getByLabel("日志日期（服务器时区）", { exact: true });
      await expect(logDate).toHaveValue("2032-12-31");
      await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(2);
      await expect(page.getByRole("table")).toContainText("2032-12-31 09:00:00 +08:00");
      await logDate.fill("2032-12-30");
      await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(1);
      await page.getByRole("button", { name: "详细信息", exact: true }).click();
      await checkWebLanguage(page, {"routes":[["instances","Instance list"],[`details/${selectedId}`,"Details"],[`logs/${selectedId}`,"Logs"]],"names":["验收主机","测试主机"]});
      const pendingName = "Draft through rebind";
      await page.getByLabel("实例名称", { exact: true }).fill(pendingName);
      reboundId = "018f1f4b-7a5d-7b5f-8d31-000000000051";
      await page.getByRole("group", { name: "全局操作" }).getByRole("button", { name: "刷新", exact: true }).click();
      await expect.poll(() => typeof releaseReboundDetail).toBe("function");
      await expect(page.getByRole("heading", { name: "Host-50", exact: true })).toHaveCount(0);
      await expect(page.getByLabel("实例名称", { exact: true })).toHaveValue(pendingName);
      releaseReboundDetail();
      await expect(page.getByRole("heading", { name: "Rebound Host", exact: true })).toBeVisible();
      await expect(page.getByLabel("实例名称", { exact: true })).toHaveValue(pendingName);
      await expect(page.getByText("服务端仍在收到上报，但当前展示的指标采集时间明显较早；客户端可能正在补传或已调整时钟。", { exact: true })).toBeVisible();
      futureSample = true;
      await page.getByRole("group", { name: "全局操作" }).getByRole("button", { name: "刷新", exact: true }).click();
      await expect(page.getByText("指标采集时间明显晚于服务端接收时间；客户端时钟可能偏快，后续指标可能暂时不更新。", { exact: true })).toBeVisible();
      await expect(page.getByText("服务端仍在收到上报，但当前展示的指标采集时间明显较早；客户端可能正在补传或已调整时钟。", { exact: true })).toHaveCount(0);
      await page.getByRole("button", { name: "删除实例", exact: true }).click();
      await page.getByRole("dialog", { name: "删除监控实例", exact: true }).getByRole("button", { name: "确认", exact: true }).click();
      await expect.poll(() => new URL(page.url()).hash).toBe("#instances");
      await expect(page.getByRole("link", { name: "选择实例 Host-50", exact: true })).toHaveCount(0);
      await checkHeaderLogout(page, session.csrf_token);
      assert.deepEqual(errors, []);
      console.log(`${engine.name()}: current Host build, complete ordered list, full details and mobile light/dark WCAG AA passed`);
      await context.close();
    } finally { await browser.close(); }
  }
} finally { await new Promise((resolve, reject) => server.httpServer.close(error => error ? reject(error) : resolve())); }
