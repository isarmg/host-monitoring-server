import { createAdministratorApiClient } from "@sarmg/admin-web";
import { isErrorEnvelope, type ErrorEnvelope } from "@sarmg/contracts";
import { isApiClientError } from "@sarmg/http-client";

export const CURRENT_API_PREFIX = "/api/v2";

// Foundation owns administrator Session, Cookie, persistence and CSRF policy.
export const administratorApi = createAdministratorApiClient({
  baseUrl: globalThis.location.href,
});

export type Capability = {
  name: string;
  available: boolean;
  source: string;
  error_kind:
    | "unsupported"
    | "not_present"
    | "driver_missing"
    | "permission_denied"
    | "transient"
    | "invalid_data"
    | null;
  message: string | null;
};

export type JsonInteger = number | string;
export type HardwareSnapshot = {
  collected_at: string;
  cpu: Record<string, unknown>;
  networks: Record<string, unknown>[];
  sensors: Record<string, unknown>[];
  disk_health: Record<string, unknown>[];
};
export type ClientReport = {
  schema_version: number;
  report_id: string;
  collected_at: string;
  host: { id: string; os: string; os_version: string | null; kernel_version: string | null; arch: string; client_version: string };
  interval_seconds: number;
  system: {
    hardware?: HardwareSnapshot;
    uptime_seconds: JsonInteger;
    cpu: Record<string, unknown>;
    memory: Record<string, unknown>;
    networks: Record<string, unknown>[];
    disks: Record<string, unknown>[];
    temperatures: Record<string, unknown>[];
    gpus: Record<string, unknown>[];
  };
  capabilities: Capability[];
  client: { spool_pending_batches: JsonInteger; collector_errors: JsonInteger };
};

export type HostDetailResponse = { host: Host; latest: ClientReport | null };
export type HistoryPoint = {
  report_id: string; collected_at: string; received_at: string;
  cpu_usage_percent: number | null; memory_usage_percent: number | null;
  network_received_bytes_per_second: number | null; network_transmitted_bytes_per_second: number | null;
  disk_read_bytes_per_second: number | null; disk_written_bytes_per_second: number | null;
  max_temperature_celsius: number | null; gpu_utilization_percent: number | null; gpu_memory_usage_percent: number | null;
  cpu_frequency_mhz: number | null;
  gpu_power_watts: number | null;
  gpu_core_clock_mhz: number | null;
  max_fan_rpm: number | null;
  max_disk_temperature_celsius: number | null;
  max_disk_percentage_used: number | null;

};
export type HistoryResponse = { host_id: string; points: HistoryPoint[] };
export type MetricAggregate = { count: number; min: number | null; max: number | null; avg: number | null };
export type HistoryBucket = {
  start: string; end: string; sample_count: number;
  cpu_usage_percent: MetricAggregate; memory_usage_percent: MetricAggregate;
  network_received_bytes_per_second: MetricAggregate; network_transmitted_bytes_per_second: MetricAggregate;
  disk_read_bytes_per_second: MetricAggregate; disk_written_bytes_per_second: MetricAggregate;
  max_temperature_celsius: MetricAggregate; gpu_utilization_percent: MetricAggregate; gpu_memory_usage_percent: MetricAggregate;
  cpu_frequency_mhz: MetricAggregate;
  gpu_power_watts: MetricAggregate;
  gpu_core_clock_mhz: MetricAggregate;
  max_fan_rpm: MetricAggregate;
  max_disk_temperature_celsius: MetricAggregate;
  max_disk_percentage_used: MetricAggregate;

};
export type HistorySeriesResponse = {
  host_id: string; requested_from: string; requested_to: string; actual_from: string; actual_to: string;
  step_seconds: number; source: "raw" | "hourly" | "mixed"; points: HistoryBucket[];
};

export type Host = {
  id: string;
  name: string;
  os: string;
  os_version: string | null;
  kernel_version: string | null;
  arch: string;
  client_version: string;
  registered_at: string;
  last_seen_at: string;
  latest_collected_at: string | null;
  status: string;
  capabilities: Capability[];
  cpu_usage_percent: number | null;
  memory_usage_percent: number | null;
  network_received_bytes_per_second: number | null;
  network_transmitted_bytes_per_second: number | null;
  disk_read_bytes_per_second: number | null;
  disk_written_bytes_per_second: number | null;
  max_temperature_celsius: number | null;
  gpu_utilization_percent: number | null;
  gpu_memory_usage_percent: number | null;
  cpu_frequency_mhz: number | null;
  gpu_power_watts: number | null;
  gpu_core_clock_mhz: number | null;
  max_fan_rpm: number | null;
  max_disk_temperature_celsius: number | null;
  max_disk_percentage_used: number | null;

};

export type HostListResponse = {
  hosts: Host[];
  statistics: HostStatistics;
};
export type HostCount = { total: number; online: number };
export type HostStatistics = { total: HostCount; windows: HostCount; linux: HostCount; macos: HostCount };

const HOST_KEYS = [
  "id",
  "name",
  "os",
  "os_version",
  "kernel_version",
  "arch",
  "client_version",
  "registered_at",
  "last_seen_at",
  "latest_collected_at",
  "status",
  "capabilities",
  "cpu_usage_percent",
  "memory_usage_percent",
  "network_received_bytes_per_second",
  "network_transmitted_bytes_per_second",
  "disk_read_bytes_per_second",
  "disk_written_bytes_per_second",
  "max_temperature_celsius",
  "gpu_utilization_percent",
  "gpu_memory_usage_percent",
  "cpu_frequency_mhz",
  "gpu_power_watts",
  "gpu_core_clock_mhz",
  "max_fan_rpm",
  "max_disk_temperature_celsius",
  "max_disk_percentage_used",

] as const;

const CAPABILITY_KEYS = [
  "name",
  "available",
  "source",
  "error_kind",
  "message",
] as const;

const CAPABILITY_ERROR_KINDS = new Set([
  "unsupported",
  "not_present",
  "driver_missing",
  "permission_denied",
  "transient",
  "invalid_data",
]);

export function isHostListResponse(value: unknown): value is HostListResponse {
  if (!isRecordWithExactKeys(value, ["hosts", "statistics"])) {
    return false;
  }
  return (
    Array.isArray(value.hosts) &&
    value.hosts.every(isHost) && isHostStatistics(value.statistics)
  );
}

function isHostStatistics(value: unknown): value is HostStatistics {
  if (!isRecordWithExactKeys(value, ["total", "windows", "linux", "macos"])) return false;
  return [value.total, value.windows, value.linux, value.macos].every(count =>
    isRecordWithExactKeys(count, ["total", "online"])
    && isNonNegativeSafeInteger(count.total) && isNonNegativeSafeInteger(count.online)
    && count.online <= count.total);
}

export function isHostDetailResponse(value: unknown): value is HostDetailResponse {
  return isRecordWithExactKeys(value, ["host", "latest"]) && isHost(value.host)
    && (value.latest === null || isClientReport(value.latest));
}

export function isHistoryResponse(value: unknown): value is HistoryResponse {
  const metricKeys = HOST_KEYS.slice(12);
  return isRecordWithExactKeys(value, ["host_id", "points"]) && isUuid(value.host_id)
    && Array.isArray(value.points) && value.points.length <= 1000
    && value.points.every(point => isRecordWithExactKeys(point, ["report_id", "collected_at", "received_at", ...metricKeys])
      && isUuid(point.report_id) && isUtcTimestamp(point.collected_at) && isUtcTimestamp(point.received_at)
      && metricKeys.every(key => isNullableFiniteNumber(point[key])));
}

export function isHistorySeriesResponse(value: unknown): value is HistorySeriesResponse {
  const metricKeys = HOST_KEYS.slice(12);
  return isRecordWithExactKeys(value, ["host_id", "requested_from", "requested_to", "actual_from", "actual_to", "step_seconds", "source", "points"])
    && isUuid(value.host_id) && isUtcTimestamp(value.requested_from) && isUtcTimestamp(value.requested_to)
    && isUtcTimestamp(value.actual_from) && isUtcTimestamp(value.actual_to) && isPositiveSafeInteger(value.step_seconds)
    && ["raw", "hourly", "mixed"].includes(String(value.source)) && Array.isArray(value.points) && value.points.length <= 1000
    && value.points.every(point => isRecordWithExactKeys(point, ["start", "end", "sample_count", ...metricKeys])
      && isUtcTimestamp(point.start) && isUtcTimestamp(point.end) && isPositiveSafeInteger(point.sample_count)
      && metricKeys.every(key => isMetricAggregate(point[key])));
}

function isMetricAggregate(value: unknown): value is MetricAggregate {
  return isRecordWithExactKeys(value, ["count", "min", "max", "avg"]) && isNonNegativeSafeInteger(value.count)
    && isNullableFiniteNumber(value.min) && isNullableFiniteNumber(value.max) && isNullableFiniteNumber(value.avg);
}

export function isHost(value: unknown): value is Host {
  if (!isRecordWithExactKeys(value, HOST_KEYS)) return false;
  return (
    isUuid(value.id) &&
    isText(value.name) &&
    isText(value.os) &&
    isNullableText(value.os_version) &&
    isNullableText(value.kernel_version) &&
    isText(value.arch) &&
    isText(value.client_version) &&
    isUtcTimestamp(value.registered_at) &&
    isUtcTimestamp(value.last_seen_at) &&
    (value.latest_collected_at === null ||
      isUtcTimestamp(value.latest_collected_at)) &&
    isText(value.status) &&
    Array.isArray(value.capabilities) &&
    value.capabilities.every(isCapability) &&
    isNullableFiniteNumber(value.cpu_usage_percent) &&
    isNullableFiniteNumber(value.memory_usage_percent) &&
    isNullableFiniteNumber(value.network_received_bytes_per_second) &&
    isNullableFiniteNumber(value.network_transmitted_bytes_per_second) &&
    isNullableFiniteNumber(value.disk_read_bytes_per_second) &&
    isNullableFiniteNumber(value.disk_written_bytes_per_second) &&
    isNullableFiniteNumber(value.max_temperature_celsius) &&
    isNullableFiniteNumber(value.gpu_utilization_percent) &&
    isNullableFiniteNumber(value.cpu_frequency_mhz)
    && isNullableFiniteNumber(value.gpu_power_watts)
    && isNullableFiniteNumber(value.gpu_core_clock_mhz)
    && isNullableFiniteNumber(value.max_fan_rpm)
    && isNullableFiniteNumber(value.max_disk_temperature_celsius)
    && isNullableFiniteNumber(value.max_disk_percentage_used) &&
    isNullableFiniteNumber(value.gpu_memory_usage_percent)
  );
}

function isCapability(value: unknown): value is Capability {
  if (!isRecordWithExactKeys(value, CAPABILITY_KEYS)) return false;
  return (
    isText(value.name) &&
    typeof value.available === "boolean" &&
    isText(value.source) &&
    (value.error_kind === null ||
      (typeof value.error_kind === "string" &&
        CAPABILITY_ERROR_KINDS.has(value.error_kind))) &&
    isNullableText(value.message)
  );
}

function isClientReport(value: unknown): value is ClientReport {
  if (!isRecordWithExactKeys(value, ["schema_version", "report_id", "collected_at", "host", "interval_seconds", "system", "capabilities", "client"])) return false;
  if (value.schema_version !== 2 || !isUuid(value.report_id) || !isUtcTimestamp(value.collected_at)
      || typeof value.interval_seconds !== "number" || !Number.isFinite(value.interval_seconds)) return false;
  if (!isRecordWithExactKeys(value.host, ["id", "os", "os_version", "kernel_version", "arch", "client_version"])
      || !isUuid(value.host.id) || !isText(value.host.os) || !isNullableText(value.host.os_version)
      || !isNullableText(value.host.kernel_version) || !isText(value.host.arch) || !isText(value.host.client_version)) return false;
  if (!isRecord(value.system)) return false;
  const systemKeys = ["uptime_seconds", "cpu", "memory", "networks", "disks", "temperatures", "gpus"];
  if (Object.hasOwn(value.system, "hardware")) {
    systemKeys.push("hardware");
    const h = value.system.hardware;
    if (!isRecordWithExactKeys(h, ["collected_at", "cpu", "networks", "sensors", "disk_health"])
      || !isUtcTimestamp(h.collected_at) || !isRecord(h.cpu)
      || !Array.isArray(h.networks) || !h.networks.every(isRecord)
      || !Array.isArray(h.sensors) || !h.sensors.every(isRecord)
      || !Array.isArray(h.disk_health) || !h.disk_health.every(isRecord)) return false;
  }
  if (!isRecordWithExactKeys(value.system, systemKeys)
      || !isJsonInteger(value.system.uptime_seconds) || !isRecord(value.system.cpu) || !isRecord(value.system.memory)
      || !Array.isArray(value.system.networks) || !value.system.networks.every(isRecord)
      || !Array.isArray(value.system.disks) || !value.system.disks.every(isRecord)
      || !Array.isArray(value.system.temperatures) || !value.system.temperatures.every(isRecord)
      || !Array.isArray(value.system.gpus) || !value.system.gpus.every(isRecord)) return false;
  return Array.isArray(value.capabilities) && value.capabilities.every(isCapability)
    && isRecordWithExactKeys(value.client, ["spool_pending_batches", "collector_errors"])
    && isJsonInteger(value.client.spool_pending_batches) && isJsonInteger(value.client.collector_errors);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isJsonInteger(value: unknown): value is JsonInteger {
  return isNonNegativeSafeInteger(value) || (typeof value === "string" && /^(?:0|[1-9][0-9]*)$/.test(value));
}

function isRecordWithExactKeys(
  value: unknown,
  keys: readonly string[],
): value is Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return false;
  }
  const actual = Object.keys(value);
  return (
    actual.length === keys.length &&
    keys.every((key) => Object.hasOwn(value, key))
  );
}

function isText(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

function isNullableText(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isNullableFiniteNumber(value: unknown): value is number | null {
  return value === null || (typeof value === "number" && Number.isFinite(value));
}

function isNonNegativeSafeInteger(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) >= 0;
}

function isPositiveSafeInteger(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) >= 1;
}

export function isUuid(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
      value,
    )
  );
}

function isUtcTimestamp(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z$/.test(value) &&
    Number.isFinite(Date.parse(value))
  );
}

// Exposes only the validated current Foundation envelope. Callers must branch
// on `code`, never on localized/display `message` text.
export function errorEnvelope(error: unknown): ErrorEnvelope | undefined {
  if (!isApiClientError(error) || !isErrorEnvelope(error.envelope)) return undefined;
  return error.envelope;
}

export type ClientInstance = {
  request_id: string; instance_id: string; display_name: string;
  status: "pending" | "active" | "cancelled";
  created_at: string;
  authorization_code: string;
};
export type CreatedInstance = ClientInstance & { activation_code: string };
export type ClientInstanceListResponse = {
  instances: ClientInstance[];
  hosts: Host[];
};
const INSTANCE_KEYS = ["request_id", "instance_id", "display_name", "status", "created_at", "authorization_code"];
const isAuthorizationCode = (value: unknown): value is string => typeof value === "string"
  && /^[a-z0-9]{36}$/.test(value);
function instanceFields(value: Record<string, unknown>): boolean {
  return isUuid(value.request_id) && isUuid(value.instance_id) && isText(value.display_name)
    && ["pending", "active", "cancelled"].includes(String(value.status))
    && isUtcTimestamp(value.created_at) && isAuthorizationCode(value.authorization_code);
}
export function isInstances(value: unknown): value is ClientInstanceListResponse {
  return isRecordWithExactKeys(value, ["instances", "hosts"])
    && Array.isArray(value.instances)
    && value.instances.every(item => isRecordWithExactKeys(item, INSTANCE_KEYS) && instanceFields(item))
    && Array.isArray(value.hosts) && value.hosts.length <= value.instances.length && value.hosts.every(isHost);
}
export function isInstance(value: unknown): value is ClientInstance {
  return isRecordWithExactKeys(value, INSTANCE_KEYS) && instanceFields(value);
}
export function isCreatedInstance(value: unknown): value is CreatedInstance {
  return isRecordWithExactKeys(value, [...INSTANCE_KEYS, "activation_code"]) && instanceFields(value)
    && value.status === "pending" && typeof value.activation_code === "string" && /^[a-z0-9]{36}$/.test(value.activation_code);
}
export type PairingSummary = { request_id: string; os: string; arch: string; client_version: string; status: "waiting" | "active" | "denied" | "expired"; expires_at: string };
export function isPairingSummary(value: unknown): value is PairingSummary {
  return isRecordWithExactKeys(value, ["request_id", "os", "arch", "client_version", "status", "expires_at"])
    && isUuid(value.request_id) && isText(value.os) && isText(value.arch) && isText(value.client_version)
    && ["waiting", "active", "denied", "expired"].includes(String(value.status)) && isUtcTimestamp(value.expires_at);
}
export function isActivation(value: unknown): value is { instance_id: string; status: "active" } {
  return isRecordWithExactKeys(value, ["instance_id", "status"]) && isUuid(value.instance_id) && value.status === "active";
}
export function isNoContent(value: unknown): value is undefined { return value === undefined; }
