import { rangeEditor, checkDateRangeValidation } from "./date-range.mjs";
import { checkWebLanguage } from "./language.mjs";
import "./pagination-contract.mjs";
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
    status: index === 49 ? "unavailable" : index === 48 ? "stale" : "online", data_error: index === 49 ? "stored_host_data_invalid" : null, capabilities: [], cpu_usage_percent: null, memory_usage_percent: 25,
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
function listPage(values, url, prefix) {
  assert.equal(url.searchParams.has("limit"), false);
  const cursor = url.searchParams.get("cursor");
  assert.ok(cursor === null || cursor === `${prefix}_next` || cursor === `${prefix}_previous`);
  const offset = cursor === `${prefix}_next` ? 50 : 0;
  return { values: values.slice(offset, offset + 50),
    next_cursor: offset + 50 < values.length ? `${prefix}_next` : null,
    previous_cursor: offset > 0 ? `${prefix}_previous` : null };
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
  return { schema_version: 1, report_id: collectedAt.endsWith("00:14:00Z") ? "038f1f4b-7a5d-7b5f-8d31-000000000051" : "038f1f4b-7a5d-7b5f-8d31-000000000050", collected_at: collectedAt, host: { id: hostId, os: "linux", os_version: null, kernel_version: null, arch: "x86_64", client_version: "0.8.1" }, interval_seconds: 5,
    system: { hardware: { inventory_collected_at: "2026-09-04T00:00:00Z",
      memory_modules: [{id:"smbios:0001",locator:"DIMM A1",model:"M323R2GA3BB0",vendor:"Samsung",memory_type:"DDR5",module_version:"FW1.2",form_factor:"DIMM",capacity_bytes:8589934592,speed_mt_s:5600,configured_speed_mt_s:4800,reported_speed:null,source:"linux-smbios"}],
      devices: [
        {id:"tb0",kind:"thunderbolt",name:"Intel Thunderbolt 4 NHI",vendor:"Intel",source:"linux-sysfs"},
        {id:"monitor0",kind:"monitor",name:"DELL U2723QE",model:"U2723QE",source:"linux-drm-edid"},
        {id:"bt0",kind:"bluetooth",name:"Intel AX211 Bluetooth",source:"linux-sysfs"},
        {id:"usb0",kind:"usb_controller",name:"Intel USB 3.2 Controller",vendor_id:"8086",product_id:"7a60",source:"linux-sysfs"},
        {id:"usb1",kind:"usb_device",name:"USB Audio DAC",speed_mbps:480,source:"linux-sysfs"},
        {id:"audio0",kind:"audio",name:"Realtek ALC295",model:"ALC295",vendor:"Realtek",source:"linux-alsa"},
      ], collected_at: "2026-09-04T00:00:00Z", cpu: {model:"Modern CPU",frequency_mhz:4200,per_core_frequency_mhz:[4200,null],load_average:[0.1,0.2,0.3]},
      networks: [{name:"eth0",ip_addresses:["192.0.2.1/24"],link_speed_mbps:2500}, {name:"aux0",ip_addresses:["198.51.100.1/24"],link_speed_mbps:1000}],
      physical_networks: [{id:"pci-0000:03:00.0",name:"Intel I225-V",interface_name:"eth0",mac_address:"02:00:00:00:00:01",link_speed_mbps:2500,source:"linux-sysfs-net-device"}],
      sensors: [{id:"fan1",label:"CPU Fan",kind:"fan_rpm",value:1200,source:"linux-hwmon"}],
      disk_health: [{device:"/dev/nvme0",model:"NVMe SSD",healthy:false,percentage_used:105,media_errors:"9007199254740993",collected_at:"2026-09-04T00:00:00Z",source:"smartctl-json"}] }, uptime_seconds: 90061, cpu: { usage_percent: 12.5, logical_count: 8, physical_count: 4, per_core_percent: [10, 15] }, memory: { total_bytes: 17179869184, used_bytes: 8589934592, available_bytes: 8589934592, swap_total_bytes: 0, swap_used_bytes: 0 },
      networks: [{ name: "eth0", received_bytes_total: 1024, transmitted_bytes_total: 2048, received_bytes_per_second: 128, transmitted_bytes_per_second: 256, packets_received_total: 10, packets_transmitted_total: 20, receive_errors_total: 0, transmit_errors_total: 0 }],
      disks: [{ name: "nvme0n1", mount_point: "/", file_system: "ext4", total_bytes: 1000000000, available_bytes: 500000000, read_bytes_total: 4096, written_bytes_total: 8192, read_bytes_per_second: 512, written_bytes_per_second: 1024, is_read_only: false },
        { name: "large-disk", mount_point: "/large", file_system: "xfs", total_bytes: "18014398509481984", available_bytes: "9007199254740992", read_bytes_total: "18446744073709551615", written_bytes_total: 0, read_bytes_per_second: 0, written_bytes_per_second: 0, is_read_only: false }],
      temperatures: [{ id: "cpu", label: "CPU Package", celsius: 48.5, max_celsius: 90, critical_celsius: 100, source: "sysfs" }],
      gpus: [{ id: "gpu0", vendor: "NVIDIA", name: "RTX", utilization_percent: 35, memory_total_bytes: 8589934592, memory_used_bytes: 4294967296, temperature_celsius: 55, power_watts: 120, core_clock_mhz: 1500, memory_clock_mhz: 7000, pcie_rx_bytes_per_second: 256, pcie_tx_bytes_per_second: 128, source: "nvml" },
      { id: "luid_00000000_0000a1bb", vendor: "amd", name: "AMD Radeon(TM) Vega 8 Graphics", utilization_percent: 40.3, memory_total_bytes: 1010 * 1024 * 1024, memory_used_bytes: 650 * 1024 * 1024, temperature_celsius: null, power_watts: null, core_clock_mhz: null, memory_clock_mhz: null, pcie_rx_bytes_per_second: null, pcie_tx_bytes_per_second: null, source: "windows-dxgi-pdh" },
      { id: "adlx_00000300", vendor: "amd", name: "AMD Radeon(TM) Vega 8 Graphics", utilization_percent: null, memory_total_bytes: null, memory_used_bytes: null, temperature_celsius: 88, power_watts: null, core_clock_mhz: null, memory_clock_mhz: null, pcie_rx_bytes_per_second: null, pcie_tx_bytes_per_second: null, source: "amd-adlx-no-luid" },
      { id: "luid_00000000_00013789", vendor: "amd", name: "AMD Radeon(TM) Vega 8 Graphics", utilization_percent: null, memory_total_bytes: 1010 * 1024 * 1024, memory_used_bytes: null, temperature_celsius: null, power_watts: null, core_clock_mhz: null, memory_clock_mhz: null, pcie_rx_bytes_per_second: null, pcie_tx_bytes_per_second: null, source: "windows-dxgi-pdh" }] },
    capabilities: [{ name: "system.cpu", available: true, source: "sysinfo", error_kind: null, message: null }], client: { spool_pending_batches: 0, collector_errors: 0 } };
}
globalThis.location ??= { href: "http://127.0.0.1/" };
const { isHostDetailResponse } = await import("../src/api.ts");
const currentDetail = { host: host(50), latest: latestReport() };
assert.equal(isHostDetailResponse(currentDetail), true);
assert.equal(isHostDetailResponse({ ...currentDetail, latest: { ...currentDetail.latest, schema_version: 3 } }), false);
for (const key of ["inventory_collected_at", "memory_modules", "devices"]) {
  const missing = structuredClone(currentDetail);
  delete missing.latest.system.hardware[key];
  assert.equal(isHostDetailResponse(missing), false);
}
const invalidDevices = structuredClone(currentDetail);
invalidDevices.latest.system.hardware.devices = null;
assert.equal(isHostDetailResponse(invalidDevices), false);

const server = await preview({ preview: { host: "127.0.0.1", port: 0, strictPort: true } });
const address = server.httpServer.address();
assert.ok(address && typeof address === "object");
try {
  for (const engine of [chromium, firefox]) {
    const browser = await engine.launch();
    try {
      const context = await browser.newContext({ locale: "zh-CN",  viewport: { width: 360, height: 740 } });
      const page = await context.newPage();
      await page.addInitScript(() => {
        window.hostPollingDispatches = { details: 0, history: 0 };
        const originalFetch = window.fetch.bind(window);
        window.fetch = (input, options) => {
          const url = new URL(input instanceof Request ? input.url : String(input), location.href);
          if (/\/monitoring\/hosts\/[0-9a-f-]+$/.test(url.pathname)) window.hostPollingDispatches.details++;
          if (/\/monitoring\/hosts\/[0-9a-f-]+\/history$/.test(url.pathname)) window.hostPollingDispatches.history++;
          return originalFetch(input, options);
        };
      });
      const errors = [];
      const requested = [];
      const instanceRequested = [];
      const reportRequested = [];
      let detailActive = 0, detailRequests = 0, maximumDetailActive = 0, historyRequests = 0, failNextHistory = false, deleted = false, renamed = null;
      let reboundId = null, releaseReboundDetail = null, releaseFirstDetail = null, futureSample = false;
      let delayNextReports = false, releaseReports = null;
      let failInstanceRefresh = false;
      const selectedHost = () => reboundId ? { ...host(50), id: reboundId, name: "Rebound Host", last_seen_at: "2026-09-04T00:10:00Z" } : host(50);
      page.on("pageerror", error => errors.push(error.message));
      await page.route("**/api/v1/**", async route => {
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
          const { values, ...cursors } = listPage(hosts, url, "hosts");
          body = { hosts: values, ...cursors, statistics: { total: { total: hosts.length, online: hosts.length - 2 }, windows: { total: 0, online: 0 }, linux: { total: hosts.length, online: hosts.length - 2 }, macos: { total: 0, online: 0 } } };
        } else if (url.pathname.endsWith("/client-instances")) {
          instanceRequested.push(url.search);
          const indexes = Array.from({ length: 51 }, (_, index) => index).filter(index => !deleted || index !== 50);
          const all = indexes.map(index => ({ ...instance(index), ...(index === 50 ? { instance_id: selectedHost().id, ...(renamed ? { display_name: renamed } : {}) } : {}) }));
          const focused = url.searchParams.get("instance_id");
          if (focused !== null && failInstanceRefresh) {
            return route.fulfill({ status: 503, json: { code: "service_unavailable", message: "Details refresh unavailable", retryable: true, request_id: "instance-refresh-123" } });
          }
          assert.ok(focused === null || !url.searchParams.has("cursor"));
          const { values, ...cursors } = focused === null ? listPage(all, url, "instances")
            : { values: all.filter(item => item.request_id === focused || item.instance_id === focused), next_cursor: null, previous_cursor: null };
          body = { instances: values, ...cursors, hosts: indexes.map(index => index === 50 ? selectedHost() : host(index)).filter(item => values.some(instance => instance.instance_id === item.id)) };
        } else if (url.pathname.endsWith(`/monitoring/client-instances/${instance(50).request_id}`) && request.method() === "PATCH") {
          assert.deepEqual(request.postDataJSON(), { display_name: "Renamed Host" });
          renamed = "Renamed Host";
          return route.fulfill({ status: 204 });
        } else if (url.pathname.endsWith("/monitoring/logs/calendar")) {
          body = { today: "2032-12-31" };
        } else if (reportsMatch) {
          reportRequested.push(url.search);
          if (delayNextReports) {
            delayNextReports = false;
            await new Promise(resolve => { releaseReports = resolve; });
          }
          const date = url.searchParams.get("date") ?? url.searchParams.get("start_date");
          const end_date = url.searchParams.get("end_date");
          assert.ok(["2032-12-31", "2032-12-30"].includes(date));
          const { values, ...cursors } = listPage((date === "2032-12-31" || end_date !== null) ? Array.from({ length: 51 }, (_, index) => index + 1) : [3], url, "reports");
          body = { host_id: reportsMatch[1], date, ...(end_date ? {end_date} : {}), ...cursors, reports: values.map(index => ({
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
      await expect(page.locator(".xcss-table-scroll[aria-label='实例列表']")).toBeVisible();
      await expect(page.locator(".xcss-table-scroll[aria-label='实例统计']")).toBeVisible();
      await page.setViewportSize({ width: 1838, height: 900 });
      const spacing = await page.evaluate(() => {
        const header = document.querySelector(".xcss-page-header");
        const menu = document.querySelector(".xcss-header-navigation button[aria-pressed='true']");
        const first = document.querySelector(".xcss-table-scroll[aria-label='实例统计']");
        const firstSection = first?.closest("section");
        const second = document.querySelector(".xcss-table-scroll[aria-label='实例列表']");
        if (!header || !menu || !first || !firstSection || !second) throw new Error("Host spacing fixture is incomplete");
        // Layout spacing uses element boxes; font glyph metrics vary by browser.
        return {
          menuWithinHeader: menu.getBoundingClientRect().top >= header.getBoundingClientRect().top && menu.getBoundingClientRect().bottom <= header.getBoundingClientRect().bottom,
          menuToFirst: first.getBoundingClientRect().top - header.getBoundingClientRect().bottom,
          sectionToList: second.getBoundingClientRect().top - firstSection.getBoundingClientRect().bottom,
        };
      });
      assert.ok(spacing.menuWithinHeader && Math.abs(spacing.menuToFirst) <= 1 && Math.abs(spacing.sectionToList - 16) <= 1, JSON.stringify(spacing));
      await expect(page.locator("h2").filter({ hasText: /^(统计|实例|实例列表)$/ })).toHaveCount(0);
      const statisticsRowTops = await page.locator(".xcss-statistics-table tbody tr").evaluateAll(rows => rows.map(row => ({ label: row.querySelector("th").getBoundingClientRect().top, value: row.querySelector("td").getBoundingClientRect().top })));
      assert.equal(statisticsRowTops.length, 4);
      assert.ok(statisticsRowTops.every(row => Math.abs(row.label - statisticsRowTops[0].label) <= 1 && Math.abs(row.value - row.label) <= 1), JSON.stringify(statisticsRowTops));
      await page.setViewportSize({ width: 360, height: 740 });
      await expect(page.getByRole("button", { name: "实例列表", exact: true })).toHaveAttribute("aria-pressed", "true");
      assert.equal(await page.getByRole("button", { name: "Diagnostics", exact: true }).count(), 0);
      const table = page.getByRole("table", { name: "实例列表" });
      await expect(table.locator("tbody tr")).toHaveCount(50);
      await expect(table).toContainText("数据不可用");
      await expect(table.getByRole("row").filter({ hasText: "Host-48" })).toContainText("上报延迟");
      await expect(page.getByRole("table", { name: "实例统计" })).toContainText("51 / 49");
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
      const instancePages = page.getByRole("navigation", { name: "实例分页", exact: true });
      await expect(instancePages.getByRole("button", { name: "上一页" })).toBeDisabled();
      await instancePages.getByRole("button", { name: "下一页" }).click();
      await expect(table.locator("tbody tr")).toHaveCount(1);
      await expect(table).not.toContainText("Host-0");
      await expect(instancePages.getByRole("button", { name: "下一页" })).toBeDisabled();
      await instancePages.getByRole("button", { name: "上一页" }).click();
      await expect(table.locator("tbody tr")).toHaveCount(50);
      await instancePages.getByRole("button", { name: "下一页" }).click();
      await expect(table.locator("tbody tr")).toHaveCount(1);
      await page.getByRole("link", { name: "选择实例 Host-50", exact: true }).click();
      await expect.poll(() => detailActive).toBe(1);
      await expect.poll(() => typeof releaseFirstDetail).toBe("function");
      await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
      await expect(page.getByRole("button", { name: "详细信息", exact: true })).toHaveAttribute("aria-pressed", "true");
      const overview = page.getByRole("region", { name: "实例概览", exact: true });
      const deviceNavigation = page.getByRole("navigation", { name: "设备信息分类" });
      await expect(deviceNavigation.getByRole("button", { name: "历史趋势" })).toHaveAttribute("aria-pressed", "true");
      await expect(page.getByRole("region", { name: "历史趋势", exact: true })).toBeVisible();
      await expect(overview).toHaveCount(0);
      releaseFirstDetail();
      await expect(deviceNavigation.getByRole("button", { name: "硬件传感器", exact: true })).toBeVisible();
      await expect(deviceNavigation.getByRole("button", { name: "历史趋势" })).toHaveAttribute("aria-pressed", "true");
      await expect(overview).toHaveCount(0);
      await deviceNavigation.getByRole("button", { name: "实例概览" }).click();
      const pairingDetails = page.getByRole("region", { name: "配对账户信息" });
      await expect(pairingDetails).toContainText(host(50).id);
      await expect(pairingDetails).toContainText(String(50).padStart(36, "0"));
      await page.getByLabel("实例名称", { exact: true }).fill("Renamed Host");
      await page.getByLabel("实例名称", { exact: true }).press("Enter");
      await expect(pairingDetails).toContainText("Renamed Host");
      const notification = page.getByRole("region", { name: "通知" });
      if (await notification.count()) await notification.getByRole("button", { name: "关闭通知" }).click();
      await expect(overview).toBeVisible();
      const detailsPanel = page.getByRole("region", { name: "实例详情" });
      await expect(detailsPanel.locator(":scope > section")).toHaveCount(1);
      await expect(detailsPanel.locator(":scope > nav")).toHaveCount(0);
      await expect(page.locator(".host-details-layout > nav")).toHaveCount(1);
      await expect(detailsPanel.getByRole("heading", { name: "最新设备信息", exact: true })).toHaveCount(0);
      assert.deepEqual([...new Set(requested)], [""]);
      assert.deepEqual([...new Set(instanceRequested)], ["", "?cursor=instances_next", "?cursor=instances_previous", `?instance_id=${instance(50).request_id}`]);
      await expect(deviceNavigation.getByRole("button", { name: "实例概览" })).toHaveAttribute("aria-pressed", "true");
      await expect(deviceNavigation.getByRole("button", { name: "历史趋势" })).toHaveAttribute("aria-pressed", "false");
      await expect(page.locator("#host-device-content .host-history-card")).toHaveCount(0);
      await deviceNavigation.getByRole("button", { name: "历史趋势" }).click();
      assert.equal(await deviceNavigation.locator("button").first().textContent(), "历史趋势");
      const menuTextOffset = await page.evaluate(() => {
        const menu = document.querySelector(".host-device-navigation");
        const firstButton = document.querySelector(".host-device-navigation > button");
        if (!menu || !firstButton) throw new Error("Host detail menu is incomplete");
        const label = document.createRange();
        label.selectNodeContents(firstButton);
        return label.getBoundingClientRect().left - menu.getBoundingClientRect().left;
      });
      assert.ok(Math.abs(menuTextOffset) < 1, `Host detail menu text is offset by ${menuTextOffset}px`);
      assert.equal(await deviceNavigation.evaluate(menu => getComputedStyle(menu).borderTopWidth), "0px");
      await page.setViewportSize({ width: 1838, height: 900 });
      const layoutGaps = await page.evaluate(() => {
        const header = document.querySelector(".xcss-page-header");
        const primary = document.querySelector(".xcss-header-navigation button[aria-pressed='true']");
        const secondary = document.querySelector(".host-device-navigation button[aria-pressed='true']");
        const panel = document.querySelector(".host-details-panel");
        const toolbar = document.querySelector(".host-history-toolbar");
        const chart = document.querySelector(".host-history-card");
        if (!header || !primary || !secondary || !panel || !toolbar || !chart) throw new Error("Host detail menu layout is incomplete");
        const textRect = element => { const range = document.createRange(); range.selectNodeContents(element); return range.getBoundingClientRect(); };
        const first = textRect(primary), second = textRect(secondary);
        const menuBottom = Math.max(...Array.from(document.querySelectorAll(".host-device-navigation button"), button => textRect(button).bottom));
        return {
          expected: parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--xcss-content-spacing")),
          gaps: [first.top - header.getBoundingClientRect().top,
            second.top - first.bottom,
            panel.getBoundingClientRect().top - menuBottom,
            chart.getBoundingClientRect().top - toolbar.getBoundingClientRect().bottom],
        };
      });
      assert.ok(layoutGaps.gaps.every(gap => Math.abs(gap - layoutGaps.expected) <= 1), `Host visible menu gaps differ: ${JSON.stringify(layoutGaps)}`);
      await page.setViewportSize({ width: 360, height: 740 });
      const wrappedMenus = await page.evaluate(() => {
        const primary = document.querySelector(".xcss-header-navigation");
        const secondary = document.querySelector(".host-device-navigation");
        if (!primary || !secondary) throw new Error("Host menus are missing");
        return {
          primaryWrap: getComputedStyle(primary).flexWrap,
          secondaryWrap: getComputedStyle(secondary).flexWrap,
          primaryFits: primary.scrollWidth <= primary.clientWidth + 1,
          secondaryFits: secondary.scrollWidth <= secondary.clientWidth + 1,
          secondaryRows: new Set([...secondary.children].map(item => item.getBoundingClientRect().top)).size,
        };
      });
      assert.equal(wrappedMenus.primaryWrap, "wrap");
      assert.equal(wrappedMenus.secondaryWrap, "wrap");
      assert.ok(wrappedMenus.primaryFits && wrappedMenus.secondaryFits, JSON.stringify(wrappedMenus));
      assert.ok(wrappedMenus.secondaryRows > 1, JSON.stringify(wrappedMenus));
      const crowdedPrimary = await page.locator(".xcss-header-navigation").evaluate(menu => {
        const extras = Array.from({ length: 4 }, () => {
          const item = menu.firstElementChild.cloneNode(true);
          item.textContent = "额外菜单";
          menu.append(item);
          return item;
        });
        const result = {
          rows: new Set([...menu.children].map(item => item.getBoundingClientRect().top)).size,
          fits: menu.scrollWidth <= menu.clientWidth + 1,
        };
        extras.forEach(item => item.remove());
        return result;
      });
      assert.ok(crowdedPrimary.rows > 1 && crowdedPrimary.fits, JSON.stringify(crowdedPrimary));
      assert.equal(await deviceNavigation.locator("button").nth(1).textContent(), "实例概览");
      assert.equal(await deviceNavigation.locator("button").nth(2).textContent(), "CPU");
      assert.equal(await deviceNavigation.locator("button").nth(3).textContent(), "内存");
      await expect(deviceNavigation.getByRole("button", { name: "概览", exact: true })).toHaveCount(0);
      assert.equal(await deviceNavigation.locator("button").last().textContent(), "监控状态");
      await expect(deviceNavigation.getByRole("button", { name: "历史趋势" })).toHaveAttribute("aria-pressed", "true");
      await expect(overview).toHaveCount(0);
      await expect(page.locator("#host-device-content .host-history-card")).toHaveCount(10);
      assert.equal(await detailsPanel.evaluate(element => getComputedStyle(element).borderTopWidth), "0px");
      assert.equal(await page.locator(".host-history-toolbar").evaluate(element => getComputedStyle(element).borderTopWidth), "0px");
      assert.ok(await page.locator(".host-history-card").evaluateAll(elements => elements.every(element => getComputedStyle(element).borderTopWidth === "0px")));
      await deviceNavigation.getByRole("button", { name: "实例概览" }).click();
      assert.equal(await detailsPanel.evaluate(element => getComputedStyle(element).borderTopWidth), "0px");
      await expect(overview.locator(".host-overview > section")).toHaveCount(3);
      await expect(overview.locator(".host-overview h2, .host-overview h3")).toHaveCount(0);
      assert.ok(await overview.locator(".host-overview > section").evaluateAll(sections => sections.every(section => getComputedStyle(section).borderTopWidth === "0px")));
      await expect(pairingDetails).toContainText("Renamed Host");
      await expect(overview.getByRole("region", { name: "实例设置" })).toBeVisible();
      await expect(overview.getByRole("region", { name: "实例操作" })).toBeVisible();
      const actionTextOffset = await overview.locator(".host-instance-actions").evaluate(section => {
        const button = section.querySelector("button");
        if (!button) throw new Error("Host instance action is missing");
        const text = document.createRange(); text.selectNodeContents(button);
        return text.getBoundingClientRect().left - section.getBoundingClientRect().left;
      });
      assert.ok(Math.abs(actionTextOffset) < 1, `Host action text is offset by ${actionTextOffset}px`);
      await expect(page.locator(".host-history-card")).toHaveCount(0);
      await deviceNavigation.getByRole("button", { name: "CPU", exact: true }).click();
      await expect(deviceNavigation.getByRole("button", { name: "CPU", exact: true })).toHaveAttribute("aria-pressed", "true");
      await expect(page.locator("#host-device-content > h3")).toHaveCount(0);
      await expect(page.locator(".host-history-card")).toHaveCount(0);
      const cpuSection = page.getByRole("region", { name: "CPU", exact: true });
      await expect(cpuSection.locator(".host-device-card")).toHaveCount(1);
      assert.equal(await detailsPanel.evaluate(element => getComputedStyle(element).borderTopWidth), "0px");
      assert.equal(await cpuSection.locator(".host-device-card").evaluate(element => getComputedStyle(element).borderTopWidth), "0px");
      await expect(cpuSection).toContainText("Modern CPU");
      await expect(cpuSection.getByRole("heading", { name: "CPU", exact: true })).toHaveCount(0);
      await expect(cpuSection.getByRole("heading", { name: "客户端状态", exact: true })).toHaveCount(0);
      await deviceNavigation.getByRole("button", { name: "内存", exact: true }).click();
      const memorySection = page.getByRole("region", { name: "内存", exact: true });
      await expect(memorySection.locator(".host-device-card")).toHaveCount(2);
      assert.equal(await memorySection.locator(".host-device-card").first().evaluate(element => getComputedStyle(element).borderTopWidth), "0px");
      await expect(memorySection).toContainText("16.0 GiB");
      await expect(memorySection).toContainText("Samsung");
      await expect(memorySection).toContainText("DDR5");
      await expect(memorySection).toContainText("FW1.2");
      await expect(memorySection).toContainText("M323R2GA3BB0");
      await expect(memorySection).toContainText("5,600 MT/s");
      await expect(memorySection).toContainText("4,800 MT/s");
      for (const [category, model] of [["雷电 / USB4", "Intel Thunderbolt 4 NHI"], ["显示器", "DELL U2723QE"], ["蓝牙", "Intel AX211 Bluetooth"], ["USB 控制器", "Intel USB 3.2 Controller"], ["USB 设备", "USB Audio DAC"], ["音频设备", "Realtek ALC295"]]) {
        await deviceNavigation.getByRole("button", { name: category, exact: true }).click();
        await expect(page.getByRole("region", { name: category, exact: true })).toContainText(model);
      }
      await deviceNavigation.getByRole("button", { name: "内存", exact: true }).click();
      await expect(memorySection.getByRole("heading", { name: "内存", exact: true })).toHaveCount(0);
      await expect(memorySection.getByRole("heading", { name: "客户端状态", exact: true })).toHaveCount(0);
      await deviceNavigation.getByRole("button", { name: "显卡" }).click();
      await expect(deviceNavigation.getByRole("button", { name: "显卡" })).toHaveAttribute("aria-pressed", "true");
      await expect(page.locator("#host-device-content").getByRole("heading", { name: "CPU", exact: true })).toHaveCount(0);
      const gpuSection = page.getByRole("region", { name: "显卡", exact: true });
      await expect(gpuSection.locator(".host-device-card")).toHaveCount(2);
      const vega = gpuSection.locator(".host-device-card").filter({ has: page.getByRole("heading", { name: "AMD Radeon(TM) Vega 8 Graphics", exact: true }) });
      await expect(vega).toContainText("40.3%");
      await expect(vega).toContainText("650 MiB");
      await expect(vega).toContainText("88.0 ℃");
      await expect(vega).toContainText("luid_00000000_00013789");
      await expect(vega).toContainText("adlx_00000300");
      await deviceNavigation.getByRole("button", { name: "内存", exact: true }).click();
      await page.getByText("16.0 GiB", { exact: true }).waitFor();
      await expect(page.locator("pre")).toHaveCount(0);
      await deviceNavigation.getByRole("button", { name: "硬件传感器" }).click();
      await expect(page.getByRole("region", {name:"硬件传感器",exact:true})).toBeVisible();
      await expect(page.getByText("1,200 RPM", {exact:true})).toBeVisible();
      await deviceNavigation.getByRole("button", { name: "磁盘健康" }).click();
      await expect(page.getByText("105.0%", {exact:true})).toBeVisible();
      await expect(page.getByText("9007199254740993", {exact:true})).toBeVisible();
      await deviceNavigation.getByRole("button", { name: "网络接口" }).click();
      await expect(page.getByText("192.0.2.1/24", {exact:true})).toBeVisible();
      const networkSection = page.getByRole("region", { name: "网络接口", exact: true });
      await expect(networkSection.locator(".host-device-card")).toHaveCount(2);
      const eth0 = networkSection.locator(".host-device-card").filter({ has: page.getByRole("heading", { name: "eth0", exact: true }) });
      await expect(eth0).toContainText("192.0.2.1/24");
      await expect(eth0).toContainText("128 B/秒");
      await expect(networkSection.locator(".host-device-card").filter({ has: page.getByRole("heading", { name: "aux0", exact: true }) })).toContainText("198.51.100.1/24");
      await deviceNavigation.getByRole("button", { name: "磁盘", exact: true }).click();
      const largeDisk = page.getByRole("region", { name: "磁盘", exact: true }).locator(".host-device-card").filter({ has: page.getByRole("heading", { name: "large-disk", exact: true }) });
      await expect(largeDisk).toContainText("16.0 PiB");
      await expect(largeDisk).toContainText("8.0 PiB");
      await expect(largeDisk).toContainText("50.0%");
      await expect(largeDisk).toContainText("16.0 EiB");
      await expect(largeDisk.getByText("已使用", { exact: true })).toBeVisible();
      await deviceNavigation.getByRole("button", { name: "网络硬件" }).click();
      const physicalNetwork = page.getByRole("region", { name: "网络硬件", exact: true });
      await expect(physicalNetwork.locator(".host-device-card")).toHaveCount(1);
      await expect(physicalNetwork).toContainText("Intel I225-V");
      await expect(physicalNetwork).toContainText("pci-0000:03:00.0");
      await deviceNavigation.getByRole("button", { name: "监控状态" }).click();
      await expect(deviceNavigation.getByRole("button", { name: "监控状态" })).toHaveAttribute("aria-pressed", "true");
      const monitoringSection = page.getByRole("region", { name: "监控状态", exact: true });
      const clientStatus = monitoringSection.locator(".host-device-card");
      assert.equal(await detailsPanel.evaluate(element => getComputedStyle(element).borderTopWidth), "0px");
      assert.equal(await clientStatus.evaluate(element => getComputedStyle(element).borderTopWidth), "0px");
      await expect(clientStatus.getByRole("heading", { name: "客户端状态", exact: true })).toBeVisible();
      await expect(clientStatus).toContainText("运行时间");
      await expect(clientStatus).toContainText("待发送批次");
      await expect(page.getByRole("button", { name: "暂停自动更新（2 秒）", exact: true })).toHaveCount(0);
      await expect(page.getByRole("button", { name: "恢复自动更新", exact: true })).toHaveCount(0);
      await page.getByText("页面最近更新", { exact: true }).waitFor();
      await expect(page.getByText("服务端仍在收到上报，但当前展示的指标采集时间明显较早；客户端可能正在补传或已调整时钟。", { exact: true })).toHaveCount(0);
      await expect(page.getByText("指标采集时间明显晚于服务端接收时间；客户端时钟可能偏快，后续指标可能暂时不更新。", { exact: true })).toHaveCount(0);
      await deviceNavigation.getByRole("button", { name: "历史趋势" }).click();
      await page.getByRole("region", { name: "历史趋势", exact: true }).waitFor();
      await expect(page.locator(".host-history-toolbar h3")).toHaveCount(0);
      assert.ok(await page.locator(".host-history-toolbar button").evaluateAll(buttons => buttons.every(button => button.getBoundingClientRect().width >= 44)));
      assert.equal(await page.locator(".host-history-card").first().evaluate(element => getComputedStyle(element).aspectRatio), "3 / 2");
      const chartCard = await page.locator(".host-history-card").first().boundingBox();
      assert.ok(chartCard && Math.abs(chartCard.width / chartCard.height - 1.5) < 0.05, JSON.stringify(chartCard));
      await expect.poll(() => detailRequests).toBeGreaterThanOrEqual(2);
      assert.equal(maximumDetailActive, 1);
      await expect.poll(() => historyRequests).toBeGreaterThanOrEqual(1);
      const cpuChart = page.getByRole("img", { name: "CPU 历史图" });
      await expect(page.getByRole("heading", { name: "磁盘 I/O", exact: true })).toBeVisible();
      await expect(cpuChart.locator("polyline")).toHaveCount(1);
      await expect(cpuChart.locator("polyline")).toHaveAttribute("points", /^0,90 0\.1388.*?,85$/);
      await expect(cpuChart.locator("circle")).toHaveCount(2);
      assert.deepEqual(await cpuChart.locator("circle").evaluateAll(elements => elements.map(element => Number(element.getAttribute("cy")))), [80, 70]);
      await page.setViewportSize({ width: 1440, height: 900 });
      const desktopCharts = await page.locator(".host-history-card").evaluateAll(elements => elements.slice(0, 5).map(element => {
        const { top, width, height } = element.getBoundingClientRect();
        return { top, width, height };
      }));
      assert.equal(desktopCharts.length, 5);
      assert.ok(desktopCharts.slice(0, 4).every(card => Math.abs(card.top - desktopCharts[0].top) < 1), JSON.stringify(desktopCharts));
      assert.ok(desktopCharts[4].top > desktopCharts[0].top, JSON.stringify(desktopCharts));
      assert.ok(await page.locator(".host-history-card").evaluateAll(elements => elements.every(element => element.scrollHeight <= element.clientHeight + 1)));
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
      await page.setViewportSize({ width: 360, height: 740 });
      await deviceNavigation.getByRole("button", { name: "监控状态" }).click();
      const hiddenRequests = await page.evaluate(() => {
        Object.defineProperty(document, "hidden", { configurable: true, get: () => true });
        document.dispatchEvent(new Event("visibilitychange"));
        return { ...window.hostPollingDispatches };
      });
      await page.waitForTimeout(6_200);
      // Count client dispatches at the visibility boundary, rather than delayed
      // Playwright route callbacks for HTTP requests already sent while visible.
      assert.deepEqual(await page.evaluate(() => window.hostPollingDispatches), hiddenRequests,
        "A hidden page must stop both detail and history polling");
      const beforeManualRefresh = detailRequests;
      const beforeManualHistoryRefresh = historyRequests;
      await page.getByRole("group", { name: "全局操作" }).getByRole("button", { name: "刷新", exact: true }).click();
      await expect.poll(() => detailRequests).toBeGreaterThan(beforeManualRefresh);
      await expect.poll(() => historyRequests).toBeGreaterThan(beforeManualHistoryRefresh);
      const beforeVisibleRefresh = { details: detailRequests, history: historyRequests };
      await page.evaluate(() => {
        delete document.hidden;
        document.dispatchEvent(new Event("visibilitychange"));
      });
      await expect.poll(() => detailRequests).toBeGreaterThan(beforeVisibleRefresh.details);
      await expect.poll(() => historyRequests).toBeGreaterThan(beforeVisibleRefresh.history);
      await deviceNavigation.getByRole("button", { name: "历史趋势" }).click();
      failNextHistory = true;
      await page.getByRole("button", { name: "24h", exact: true }).click();
      await expect(page.getByRole("alert")).toContainText("history-failure-123");
      await expect(page.locator("body")).not.toContainText("SECRET history");
      await page.getByRole("alert").getByRole("button", { name: "重试", exact: true }).click();
      await expect(page.getByRole("img", { name: "CPU 历史图" })).toBeVisible();
      // A failed enclosing instance refresh must not unmount the live monitoring
      // workspace or discard its independently selected category and time range.
      await deviceNavigation.getByRole("button", { name: "CPU", exact: true }).click();
      failInstanceRefresh = true;
      await page.getByRole("group", { name: "全局操作" }).getByRole("button", { name: "刷新", exact: true }).click();
      await expect(page.getByRole("alert")).toContainText("instance-refresh-123");
      await expect(page.getByRole("alert")).toContainText("上次成功数据");
      await expect(deviceNavigation.getByRole("button", { name: "CPU", exact: true })).toHaveAttribute("aria-pressed", "true");
      await expect(cpuSection).toContainText("Modern CPU");
      failInstanceRefresh = false;
      await page.getByRole("group", { name: "全局操作" }).getByRole("button", { name: "刷新", exact: true }).click();
      await expect(page.getByRole("alert")).toHaveCount(0);
      await expect(deviceNavigation.getByRole("button", { name: "CPU", exact: true })).toHaveAttribute("aria-pressed", "true");
      await deviceNavigation.getByRole("button", { name: "历史趋势" }).click();
      await expect(page.getByRole("button", { name: "24h", exact: true })).toHaveAttribute("aria-pressed", "true");
      for (const theme of ["light", "dark"]) {
        if (await page.locator("html").getAttribute("data-theme") !== theme) await page.getByRole("button", { name: /切换到.*模式/ }).click();
        const colors = await page.evaluate(() => ({
          page: getComputedStyle(document.documentElement).backgroundColor,
          card: getComputedStyle(document.querySelector(".host-history-card")).backgroundColor,
          cardText: getComputedStyle(document.querySelector(".host-history-card")).color,
          actionText: getComputedStyle(document.querySelector('.host-history-toolbar button:not([aria-pressed="true"])')).color,
          borderWidth: getComputedStyle(document.querySelector(".host-history-card")).borderTopWidth,
        }));
        if (theme === "light") assert.deepEqual(colors, { page: "rgb(255, 255, 255)", card: "rgb(242, 242, 242)", cardText: "rgb(31, 31, 31)", actionText: "rgb(68, 68, 68)", borderWidth: "0px" });
        else assert.notEqual(colors.card, "rgb(242, 242, 242)");
        assert.equal(colors.borderWidth, "0px");
        const result = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
        assert.deepEqual(result.violations, []);
        assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
      }
      const selectedId = instance(50).request_id;
      await page.getByRole("button", { name: "日志", exact: true }).click();
      await expect(page.getByRole("button", { name: "日志", exact: true })).toHaveAttribute("aria-pressed", "true");
      await expect(page.locator(".host-logs-panel")).toBeVisible();
      await expect(page.getByRole("heading", { name: "上报日志", exact: true })).toHaveCount(0);
      await expect(page.getByText("按服务端接收日期列出全部上报。采集时间由客户端提供，可能早于接收时间。", { exact: true })).toHaveCount(0);
      await page.setViewportSize({ width: 1838, height: 900 });
      await expect.poll(() => page.evaluate(() => document.fonts.status), { timeout: 5000, message: "Host log fonts did not settle" }).toBe("loaded");
      try {
        // Wait for the logs route, fonts and resized frame before measuring glyphs.
        // The visible spacing requirement remains exactly 16 ± 1px.
        await expect.poll(() => page.evaluate(async () => {
          await new Promise(resolve => requestAnimationFrame(resolve));
          const menu = document.querySelector(".xcss-header-navigation button[aria-pressed='true']");
          const panel = document.querySelector(".host-logs-panel");
          if (!menu || !panel) throw new Error("Host log layout is incomplete");
          const text = document.createRange(); text.selectNodeContents(menu);
          const gap = panel.getBoundingClientRect().top - text.getBoundingClientRect().bottom;
          return { gap, withinSpacing: Math.abs(gap - 16) <= 1 };
        }), { timeout: 5000, message: "Host log menu spacing must be 16 ± 1px" }).toMatchObject({ withinSpacing: true });
      } catch (error) {
        console.error("Host log layout diagnostics", await page.evaluate(() => ({
          viewport: { width: innerWidth, height: innerHeight }, fonts: document.fonts.status,
          elements: [...document.querySelectorAll(".xcss-page-header,.xcss-header-navigation,.xcss-shell-main > *, #hosts > *")].map(element => ({ tag: element.tagName, className: element.className, hidden: element.hidden, display: getComputedStyle(element).display, rect: element.getBoundingClientRect().toJSON() })),
          errors: [...document.querySelectorAll("[role='alert']")].map(element => element.textContent?.replace(/SECRET[^。\n]*/g, "[redacted]").slice(0, 160)),
        })));
        throw error;
      }
      const hostLogControls = await page.evaluate(() => {
        const label = document.querySelector(".host-logs-panel .xcss-log-date-controls > label");
        const button = document.querySelector(".host-logs-panel .xcss-log-date-controls button");
        const input = document.querySelector(".host-logs-panel .xcss-log-date-controls input");
        if (!label || !button || !input) throw new Error("Host log date controls are incomplete");
        const text = element => { const range = document.createRange(); range.selectNodeContents(element); return range.getBoundingClientRect(); };
        return { textOffset: text(button).top - text(label).top, buttonRightOfLabel: button.getBoundingClientRect().left >= text(label).right, inputBelowLabel: input.getBoundingClientRect().top > label.getBoundingClientRect().bottom };
      });
      assert.ok(Math.abs(hostLogControls.textOffset) <= 1 && hostLogControls.buttonRightOfLabel && hostLogControls.inputBelowLabel, JSON.stringify(hostLogControls));
      await page.setViewportSize({ width: 360, height: 740 });
      const logDate = rangeEditor(page, "host-log-date");
      await logDate.expectValue("2032-12-31");
      await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(50);
      await expect(page.getByRole("table")).toContainText("2032-12-31 09:00:00 +08:00");
      const logPages = page.getByRole("navigation", { name: "日志分页", exact: true });
      await logPages.getByRole("button", { name: "下一页" }).click();
      await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(1);
      await expect(page.getByRole("table")).toContainText("038f1f4b-7a5d-7b5f-8d31-000000000051");
      await expect(page.getByRole("table")).not.toContainText("038f1f4b-7a5d-7b5f-8d31-000000000001");
      await logPages.getByRole("button", { name: "上一页" }).click();
      await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(50);
      await logPages.getByRole("button", { name: "下一页" }).click();
      await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(1);
      await page.getByRole("button", { name: "刷新日志", exact: true }).click();
      await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(50);
      assert.equal(new URLSearchParams(reportRequested.at(-1)).has("cursor"), false);
      delayNextReports = true;
      await logPages.getByRole("button", { name: "下一页" }).click();
      await expect.poll(() => typeof releaseReports).toBe("function");
      await logDate.fill("2032-12-30");
      await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(1);
      assert.equal(new URLSearchParams(reportRequested.at(-1)).has("cursor"), false);
      await expect(logPages.getByRole("button", { name: "上一页" })).toBeDisabled();
      releaseReports();
      await page.waitForTimeout(200);
      await expect(page.getByRole("table")).toContainText("2032-12-30 09:00:00 +08:00");
      await expect(page.getByRole("table")).not.toContainText("2032-12-31 09:00:00 +08:00");
      await checkDateRangeValidation(logDate, () => reportRequested.length, "2032-12-30", "2032-12-31");
      await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(50);
      const rangeQuery = new URLSearchParams(reportRequested.at(-1));
      assert.equal(rangeQuery.get("start_date"), "2032-12-30");
      assert.equal(rangeQuery.get("end_date"), "2032-12-31");
      assert.equal(rangeQuery.has("cursor"), false);
      const focusedRefresh = page.waitForResponse(response => new URL(response.url()).pathname.endsWith("/monitoring/client-instances") && new URL(response.url()).searchParams.has("instance_id"));
      await page.getByRole("banner").getByRole("button", { name: "刷新", exact: true }).click();
      await focusedRefresh;
      await logDate.expectValue("2032-12-30", "2032-12-31");
      await expect(page.getByRole("table")).toContainText("2032-12-30 09:00:00 +08:00");
      failInstanceRefresh = true;
      await page.getByRole("banner").getByRole("button", { name: "刷新", exact: true }).click();
      await expect(page.getByRole("alert")).toContainText("instance-refresh-123");
      await expect(page.getByRole("alert")).toContainText("上次成功读取的实例信息");
      await logDate.expectValue("2032-12-30", "2032-12-31");
      failInstanceRefresh = false;
      await page.getByRole("alert").getByRole("button", { name: "重试", exact: true }).click();
      await expect(page.getByRole("alert")).toHaveCount(0);
      await logDate.expectValue("2032-12-30", "2032-12-31");
      await expect(page.getByRole("table")).toContainText("2032-12-30 09:00:00 +08:00");
      await page.getByRole("button", { name: "详细信息", exact: true }).click();
      await deviceNavigation.waitFor();
      await expect(deviceNavigation.getByRole("button", { name: "历史趋势" })).toHaveAttribute("aria-pressed", "true");
      await expect(overview).toHaveCount(0);
      await checkWebLanguage(page, {"routes":[["instances","Instance list"],[`details/${selectedId}`,"Details"],[`logs/${selectedId}`,"Logs"]],"names":["验收主机","测试主机"]});
      const pendingName = "Draft through rebind";
      await deviceNavigation.getByRole("button", { name: "实例概览" }).click();
      await page.getByLabel("实例名称", { exact: true }).fill(pendingName);
      reboundId = "018f1f4b-7a5d-7b5f-8d31-000000000051";
      await page.getByRole("group", { name: "全局操作" }).getByRole("button", { name: "刷新", exact: true }).click();
      await expect.poll(() => typeof releaseReboundDetail).toBe("function");
      await expect(deviceNavigation.getByRole("button", { name: "历史趋势" })).toHaveAttribute("aria-pressed", "true");
      await expect(overview).toHaveCount(0);
      await deviceNavigation.getByRole("button", { name: "实例概览" }).click();
      await expect(page.getByLabel("实例名称", { exact: true })).toHaveValue(pendingName);
      releaseReboundDetail();
      await expect(deviceNavigation.getByRole("button", { name: "硬件传感器", exact: true })).toBeVisible();
      await expect(deviceNavigation.getByRole("button", { name: "实例概览" })).toHaveAttribute("aria-pressed", "true");
      await expect(page.getByLabel("实例名称", { exact: true })).toHaveValue(pendingName);
      await deviceNavigation.getByRole("button", { name: "监控状态" }).click();
      await expect(page.getByRole("heading", { name: "Rebound Host", exact: true })).toBeVisible();
      await expect(page.getByText("服务端仍在收到上报，但当前展示的指标采集时间明显较早；客户端可能正在补传或已调整时钟。", { exact: true })).toBeVisible();
      futureSample = true;
      await page.getByRole("group", { name: "全局操作" }).getByRole("button", { name: "刷新", exact: true }).click();
      await expect(page.getByText("指标采集时间明显晚于服务端接收时间；客户端时钟可能偏快，后续指标可能暂时不更新。", { exact: true })).toBeVisible();
      await expect(page.getByText("服务端仍在收到上报，但当前展示的指标采集时间明显较早；客户端可能正在补传或已调整时钟。", { exact: true })).toHaveCount(0);
      await deviceNavigation.getByRole("button", { name: "实例概览" }).click();
      await page.getByRole("button", { name: "删除实例", exact: true }).click();
      await page.getByRole("dialog", { name: "删除监控实例", exact: true }).getByRole("button", { name: "确认", exact: true }).click();
      await expect.poll(() => new URL(page.url()).hash).toBe("#instances");
      await expect(page.getByRole("link", { name: "选择实例 Host-50", exact: true })).toHaveCount(0);
      await checkHeaderLogout(page, session.csrf_token);
      assert.deepEqual(errors, []);
      console.log(`${engine.name()}: bounded single-page instances/reports, inverse cursor/date reset/stale cancellation/focused details and mobile light/dark WCAG AA passed`);
      await context.close();
    } finally { await browser.close(); }
  }
} finally { await new Promise((resolve, reject) => server.httpServer.close(error => error ? reject(error) : resolve())); }
