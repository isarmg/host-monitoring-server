import { createAdministratorApiClient } from "@sarmg/admin-web";
import { isErrorEnvelope, type ErrorEnvelope } from "@sarmg/contracts";
import { isApiClientError } from "@sarmg/http-client";

export const CURRENT_API_PREFIX = "/api/v2";

// Foundation owns administrator Session, Cookie, persistence and CSRF policy.
export const administratorApi = createAdministratorApiClient({
  baseUrl: globalThis.location.href,
});

type Capability = {
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
};

export type HostListResponse = {
  hosts: Host[];
  total: number;
  limit: number;
  offset: number;
};

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
  if (!isRecordWithExactKeys(value, ["hosts", "total", "limit", "offset"])) {
    return false;
  }
  return (
    Array.isArray(value.hosts) &&
    value.hosts.every(isHost) &&
    isNonNegativeSafeInteger(value.total) &&
    isPositiveSafeInteger(value.limit) &&
    isNonNegativeSafeInteger(value.offset)
  );
}

function isHost(value: unknown): value is Host {
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
const INSTANCE_KEYS = ["request_id", "instance_id", "display_name", "status", "created_at", "authorization_code"];
function instanceFields(value: Record<string, unknown>): boolean {
  return isUuid(value.request_id) && isUuid(value.instance_id) && isText(value.display_name)
    && ["pending", "active", "cancelled"].includes(String(value.status))
    && isUtcTimestamp(value.created_at) && typeof value.authorization_code === "string"
    && /^uci_[0-9a-f]{32}$/.test(value.authorization_code);
}
export function isInstances(value: unknown): value is ClientInstance[] {
  return Array.isArray(value) && value.length <= 200 && value.every(item => isRecordWithExactKeys(item, INSTANCE_KEYS) && instanceFields(item));
}
export function isInstance(value: unknown): value is ClientInstance {
  return isRecordWithExactKeys(value, INSTANCE_KEYS) && instanceFields(value);
}
export function isCreatedInstance(value: unknown): value is CreatedInstance {
  return isRecordWithExactKeys(value, [...INSTANCE_KEYS, "activation_code"]) && instanceFields(value)
    && value.status === "pending" && typeof value.activation_code === "string" && /^uci_[0-9a-f]{32}$/.test(value.activation_code);
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
