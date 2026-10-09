import assert from "node:assert/strict";
import { requestJson } from "@xcss/http-client";

globalThis.location ??= { href: "http://127.0.0.1/" };
const {
  isReportLogsResponse, isInstances, isFocusedInstance, isHostListResponse,
  LIST_PAGE_SIZE, LIST_REQUEST_BUDGET,
} = await import("../src/api.ts");

const identity = index => "018f1f4b-7a5d-7b5f-8d31-" + String(index).padStart(12, "0");
const report = index => ({ report_id: identity(index), collected_at: "2032-12-31T00:00:00Z",
  received_at: "2032-12-31T01:00:00Z", collected_at_server: "2032-12-31 08:00:00 +08:00",
  received_at_server: "2032-12-31 09:00:00 +08:00" });
const page = { host_id: identity(100), date: "2032-12-31", next_cursor: "bmV4dA", previous_cursor: null,
  reports: Array.from({ length: 50 }, (_, index) => report(index)) };
assert.equal(LIST_PAGE_SIZE, 50);
assert.equal(isReportLogsResponse(page), true);
assert.equal(isReportLogsResponse({ ...page, reports: [...page.reports, report(50)] }), false);
assert.equal(isReportLogsResponse({ ...page, reports: [report(1), report(1)] }), false);
assert.equal(isReportLogsResponse({ ...page, next_cursor: "A".repeat(513) }), false);
assert.equal(isReportLogsResponse({ ...page, next_cursor: "é" }), false);
assert.equal(isReportLogsResponse({ ...page, next_cursor: "not/a/cursor" }), false);
assert.equal(isReportLogsResponse({ ...page, previous_cursor: undefined }), false);
assert.equal(isReportLogsResponse({ ...page, unknown: true }), false);

const instance = index => ({ request_id: identity(index), instance_id: identity(index + 100),
  display_name: `Host-${index}`, status: "active", created_at: "2032-12-31T00:00:00Z", authorization_code: "a".repeat(36) });
const instances = { instances: Array.from({ length: 50 }, (_, index) => instance(index)), hosts: [], next_cursor: null, previous_cursor: null };
assert.equal(isInstances(instances), true);
assert.equal(isInstances({ ...instances, instances: [...instances.instances, instance(50)] }), false);
assert.equal(isInstances({ ...instances, instances: [instance(1), instance(1)] }), false);
const focused = { ...instances, instances: [instance(0)] };
assert.equal(isFocusedInstance(focused, identity(0)), true);
assert.equal(isFocusedInstance(focused, identity(100)), true);
assert.equal(isFocusedInstance(focused, identity(1)), false);
assert.equal(isFocusedInstance({ ...focused, next_cursor: "bmV4dA" }, identity(0)), false);
assert.equal(isFocusedInstance({ ...focused, instances: [instance(0), instance(1)] }, identity(0)), false);
const count = { total: 0, online: 0 };
const hosts = { hosts: [], statistics: { total: count, windows: count, linux: count, macos: count }, next_cursor: null, previous_cursor: null };
assert.equal(isHostListResponse(hosts), true);
assert.equal(isHostListResponse({ ...hosts, next_cursor: "A".repeat(513) }), false);

assert.deepEqual(LIST_REQUEST_BUDGET, { maxResponseBytes: 8 * 1024 * 1024, timeoutMs: 5_000 });
let cancelled = false;
let extra = false;
const bytes = new TextEncoder().encode(JSON.stringify(page));
await assert.rejects(requestJson("http://127.0.0.1/api/v1/monitoring/hosts", {
  ...LIST_REQUEST_BUDGET,
  fetchImpl: async () => new Response(new ReadableStream({
    start(controller) { controller.enqueue(bytes); },
    pull(controller) { if (!extra) { extra = true; controller.enqueue(new Uint8Array(LIST_REQUEST_BUDGET.maxResponseBytes + 1)); } },
    cancel() { cancelled = true; },
  }), { headers: { "content-type": "application/json" } }),
}), error => error.code === "response_too_large");
assert.equal(cancelled, true);
const began = Date.now();
await assert.rejects(requestJson("http://127.0.0.1/api/v1/monitoring/hosts", {
  ...LIST_REQUEST_BUDGET,
  fetchImpl: async (_url, { signal }) => new Promise((_resolve, reject) => {
    signal.addEventListener("abort", () => reject(signal.reason), { once: true });
  }),
}), error => error.code === "request_timeout");
assert.ok(Date.now() - began >= 4_500);
console.log("Host pagination: fixed 50 rows, bounded opaque cursors, focused identity, stream cancellation and actual 5-second timeout passed");
