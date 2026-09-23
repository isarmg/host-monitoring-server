import { displayLabel } from "./display-labels";
import { t, getLocale } from "@sarmg/admin-ui/i18n";
import { Fragment, useEffect, useRef, useState } from "react";
import { Button, ConfirmDangerDialog, ErrorState, LoadingState } from "@sarmg/admin-ui";
import { errorRequestId, useAdminApplication } from "@sarmg/admin-shell";
import {
  isHistorySeriesResponse,
  isHostDetailResponse,
  isNoContent,
  type ClientReport,
  type HistorySeriesResponse,
  type HostDetailResponse,
} from "./api";

type Failure = { requestId?: string };

export function HostDetails({ hostId, refreshSignal, removed }: { hostId: string; refreshSignal: number; removed(): void }) {
  const { client, notify } = useAdminApplication();
  const [detail, setDetail] = useState<HostDetailResponse | null>(null);
  const [history, setHistory] = useState<HistorySeriesResponse | null>(null);
  const [historyHours, setHistoryHours] = useState(1);
  const [historyLoading, setHistoryLoading] = useState(true);
  const [historyFailure, setHistoryFailure] = useState<Failure | null>(null);
  const [historyGeneration, setHistoryGeneration] = useState(0);
  const [updatedAt, setUpdatedAt] = useState<Date | null>(null);
  const [paused, setPaused] = useState(false);
  const [failure, setFailure] = useState<Failure | null>(null);
  const [pending, setPending] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const mutation = useRef<AbortController | null>(null);
  const appliedRefreshSignal = useRef(refreshSignal);
  const requestRefresh = useRef<() => void>(() => undefined);

  useEffect(() => {
    let timer: number | undefined;
    let controller: AbortController | undefined;
    let stopped = false;
    let inFlight = false;
    let refreshQueued = false;
    let forceQueued = false;
    async function refresh(force = false) {
      if (stopped || (!force && (paused || document.hidden))) return;
      if (inFlight) { refreshQueued = true; forceQueued ||= force; return; }
      inFlight = true;
      refreshQueued = false;
      forceQueued = false;
      controller = new AbortController();
      try {
        const value = await client.request(`/api/v2/monitoring/hosts/${hostId}`, isHostDetailResponse, { signal: controller.signal });
        if (stopped || controller.signal.aborted || value.host.id !== hostId) return;
        setDetail(value); setUpdatedAt(new Date()); setFailure(null);
      } catch (error) {
        if (!stopped && !controller.signal.aborted) setFailure({ requestId: errorRequestId(error) });
      } finally {
        inFlight = false;
        if (stopped) return;
        if (refreshQueued) void refresh(forceQueued);
        else if (!paused && !document.hidden) timer = window.setTimeout(refresh, 2_000);
      }
    }
    const visible = () => {
      if (document.hidden || paused) return;
      if (timer !== undefined) { clearTimeout(timer); timer = undefined; }
      if (inFlight) refreshQueued = true;
      else void refresh();
    };
    requestRefresh.current = () => {
      if (timer !== undefined) { clearTimeout(timer); timer = undefined; }
      if (inFlight) { refreshQueued = true; forceQueued = true; }
      else void refresh(true);
    };
    document.addEventListener("visibilitychange", visible);
    void refresh();
    return () => { stopped = true; requestRefresh.current = () => undefined; controller?.abort(); if (timer !== undefined) clearTimeout(timer); document.removeEventListener("visibilitychange", visible); };
  }, [client, hostId, paused]);

  useEffect(() => {
    if (appliedRefreshSignal.current === refreshSignal) return;
    appliedRefreshSignal.current = refreshSignal;
    requestRefresh.current();
  }, [refreshSignal]);

  useEffect(() => {
    let timer: number | undefined;
    let controller: AbortController | undefined;
    let stopped = false;
    setHistory(null); setHistoryLoading(true); setHistoryFailure(null);
    async function refreshHistory() {
      controller = new AbortController();
      const from = new Date(Date.now() - historyHours * 60 * 60 * 1000).toISOString();
      try {
        const value = await client.request(`/api/v2/monitoring/hosts/${hostId}/history?from=${encodeURIComponent(from)}&resolution=auto&max_points=720`, isHistorySeriesResponse, { signal: controller.signal });
        if (!stopped && !controller.signal.aborted && value.host_id === hostId) {
          setHistory(value); setHistoryFailure(null);
        }
      } catch (error) {
        if (!stopped && !controller.signal.aborted) setHistoryFailure({ requestId: errorRequestId(error) });
      } finally {
        if (!stopped && !controller.signal.aborted) {
          setHistoryLoading(false);
          timer = window.setTimeout(refreshHistory, historyHours <= 1 ? 5_000 : 30_000);
        }
      }
    }
    void refreshHistory();
    return () => { stopped = true; controller?.abort(); if (timer !== undefined) clearTimeout(timer); };
  }, [client, hostId, historyHours, historyGeneration, refreshSignal]);

  useEffect(() => () => mutation.current?.abort(), []);
  async function remove() {
    if (mutation.current) return;
    const controller = new AbortController(); mutation.current = controller; setPending(true); setFailure(null);
    try {
      await client.request(`/api/v2/monitoring/managed-instances/${hostId}`, isNoContent, { method: "DELETE", signal: controller.signal });
      if (!controller.signal.aborted) {
        setDeleting(false);
        notify(t("实例已移除", "Instance removed"));
        removed();
      }
    } catch (error) { if (!controller.signal.aborted) setFailure({ requestId: errorRequestId(error) }); }
    finally { if (!controller.signal.aborted) { mutation.current = null; setPending(false); } }
  }
  if (detail === null) return failure ? <ErrorState requestId={failure.requestId}>{t("无法读取实例详情", "Unable to load instance details")}</ErrorState> : <LoadingState>{t("正在读取实例详情…", "Loading instance details…")}</LoadingState>;
  const { host, latest } = detail;
  return <div className="sarmg-content-stack">
    <section className="sarmg-content-panel"><h2>{host.name}</h2><dl className="host-detail-list">
      <dt>{t("状态", "Status")}</dt><dd>{displayLabel(host.status)}</dd>
      <dt>{t("系统", "System")}</dt><dd>{host.os}{host.os_version ? ` ${host.os_version}` : ""} / {host.arch}</dd>
      <dt>{t("内核版本", "Kernel version")}</dt><dd>{host.kernel_version ?? t("不可用", "Unavailable")}</dd>
      <dt>{t("客户端版本", "Client version")}</dt><dd>{host.client_version}</dd>
      <dt>{t("最新 CPU", "Latest CPU")}</dt><dd>{formatPercent(host.cpu_usage_percent)}</dd>
      <dt>{t("最新内存", "Latest memory")}</dt><dd>{formatPercent(host.memory_usage_percent)}</dd>
      <dt>{t("数据采集时间", "Data collected at")}</dt><dd>{formatTime(host.latest_collected_at)}</dd>
      <dt>{t("服务端最近收到", "Last received by server")}</dt><dd>{formatTime(host.last_seen_at)}</dd>
      <dt>{t("页面最近更新", "Page last updated")}</dt><dd>{updatedAt?.toLocaleString(getLocale()) ?? "—"}</dd>
    </dl><div className="sarmg-actions"><Button aria-pressed={paused} onClick={() => setPaused(value => !value)}>{paused ? t("恢复自动更新", "Resume automatic updates") : t("暂停自动更新（2 秒）", "Pause automatic updates (2s)")}</Button></div>
      {failure && <ErrorState requestId={failure.requestId}>{t("自动更新暂时失败，页面保留上次成功数据。", "Automatic update failed temporarily. The last successful data is retained.")}</ErrorState>}
    </section>
    <HistoryChart response={history} hours={historyHours} loading={historyLoading} failure={historyFailure} retry={() => setHistoryGeneration(value => value + 1)} changeHours={setHistoryHours} />
    <LatestDevices report={latest} />
    <section className="sarmg-content-panel" aria-label={t("实例操作", "Instance actions")}><h2>{t("实例操作", "Instance actions")}</h2><div className="sarmg-actions"><Button disabled={pending} onClick={() => setDeleting(true)}>{t("删除实例", "Delete instance")}</Button></div></section>
    {deleting && <ConfirmDangerDialog title={t("删除监控实例", "Delete monitoring instance")} description={t("移除 {0} 的监控数据和绑定凭据。该客户端需要重新配对才能再次接入。", "Remove monitoring data and bound credentials for {0}. The client must pair again to reconnect.", [host.name])}
      pending={pending} onClose={() => { if (!mutation.current) setDeleting(false); }} onConfirm={() => void remove()} />}
  </div>;
}

type HistoryMetric = keyof Pick<HistorySeriesResponse["points"][number],
  "cpu_usage_percent" | "memory_usage_percent" | "disk_read_bytes_per_second" | "disk_written_bytes_per_second" | "max_temperature_celsius" | "gpu_utilization_percent" | "gpu_memory_usage_percent" | "cpu_frequency_mhz" | "gpu_power_watts" | "gpu_core_clock_mhz" | "max_fan_rpm" | "max_disk_temperature_celsius" | "max_disk_percentage_used">;
type ChartLine = { key: HistoryMetric; label: string; color: string };

function HistoryChart({ response, hours, loading, failure, retry, changeHours }: { response: HistorySeriesResponse | null; hours: number; loading: boolean; failure: Failure | null; retry(): void; changeHours(value: number): void }) {
  const charts: Array<{ title: string; unit: "percent" | "bytes" | "MHz" | "W" | "RPM" | "℃"; lines: ChartLine[] }> = [
    { title: "CPU", unit: "percent", lines: [
      { key: "cpu_usage_percent", label: t("使用率", "Usage"), color: "#2878b5" },
    ] },
    { title: "GPU", unit: "percent", lines: [
      { key: "gpu_utilization_percent", label: t("使用率", "Usage"), color: "#6f58a8" },
      { key: "gpu_memory_usage_percent", label: t("显存使用率", "Memory usage"), color: "#bd6eaa" },
    ] },
    { title: "SSD", unit: "bytes", lines: [
      { key: "disk_read_bytes_per_second", label: t("读取", "Read"), color: "#2c8c74" },
      { key: "disk_written_bytes_per_second", label: t("写入", "Write"), color: "#d28a37" },
    ] },
    { title: t("CPU 频率", "CPU frequency"), unit: "MHz", lines: [{ key: "cpu_frequency_mhz", label: t("平均频率", "Mean frequency"), color: "#2878b5" }] },
    { title: t("GPU 功耗", "GPU power"), unit: "W", lines: [{ key: "gpu_power_watts", label: t("最大设备功耗", "Maximum device power"), color: "#6f58a8" }] },
    { title: t("GPU 核心频率", "GPU core clock"), unit: "MHz", lines: [{ key: "gpu_core_clock_mhz", label: t("最大设备频率", "Maximum device frequency"), color: "#6f58a8" }] },
    { title: t("风扇转速", "Fan speed"), unit: "RPM", lines: [{ key: "max_fan_rpm", label: t("最大转速", "Maximum speed"), color: "#2c8c74" }] },
    { title: t("磁盘温度", "Disk temperature"), unit: "℃", lines: [{ key: "max_disk_temperature_celsius", label: t("最高温度", "Maximum temperature"), color: "#d28a37" }] },
    { title: t("NVMe 寿命消耗", "NVMe endurance used"), unit: "percent", lines: [{ key: "max_disk_percentage_used", label: t("最大已用比例（可超过 100%）", "Maximum used (may exceed 100%)"), color: "#d28a37" }] },
    { title: "RAM", unit: "percent", lines: [
      { key: "memory_usage_percent", label: t("使用率", "Usage"), color: "#7a5aa6" },
    ] },
  ];
  return <section className="sarmg-content-stack" aria-labelledby="history-heading"><div className="sarmg-content-panel"><h2 id="history-heading">{t("历史趋势", "History trends")}</h2>
    <div className="sarmg-actions">{[[0.25, "15m"], [1, "1h"], [6, "6h"], [24, "24h"], [168, "7d"], [720, "30d"]].map(([value, label]) => <Button key={label} aria-pressed={hours === value} onClick={() => changeHours(Number(value))}>{label}</Button>)}</div>
    <p>{response ? t("每点 {0} 秒，来源：{1}；横轴按采样时间，缺失区间不连线。", "{0} seconds per point, source: {1}; the horizontal axis uses sample time and missing intervals are not connected.", [String(response.step_seconds), response.source]) : loading ? t("正在读取历史趋势…", "Loading history trends…") : null}</p>
    {failure && <ErrorState requestId={failure.requestId} onRetry={retry}>{t("无法读取所选范围的历史趋势。", "Unable to load history trends for the selected range.")}</ErrorState>}</div>
    <div className="host-history-grid">{charts.map(chart => <MetricChartBlock key={chart.title} title={chart.title} unit={chart.unit} response={response} lines={chart.lines} loading={loading} failure={failure} />)}</div>
  </section>;
}

function MetricChartBlock({ title, unit, response, lines, loading, failure }: { title: string; unit: "percent" | "bytes" | "MHz" | "W" | "RPM" | "℃"; response: HistorySeriesResponse | null; lines: ChartLine[]; loading: boolean; failure: Failure | null }) {
  const points = response?.points ?? [];
  const requestedStart = response === null ? Number.NaN : Date.parse(response.requested_from);
  const requestedEnd = response === null ? Number.NaN : Date.parse(response.requested_to);
  const pointTimes = points.map(point => Date.parse(point.start)).filter(Number.isFinite);
  const start = Number.isFinite(requestedStart) ? requestedStart : Math.min(...pointTimes);
  const end = Number.isFinite(requestedEnd) ? requestedEnd : Math.max(...pointTimes);
  const values = points.flatMap(point => lines.map(line => point[line.key].avg).filter((value): value is number => value !== null));
  const ceiling = Math.max(unit === "percent" ? 100 : 1, ...values);
  const xAt = (value: string) => {
    const timestamp = Date.parse(value);
    if (!Number.isFinite(timestamp) || !Number.isFinite(start) || !Number.isFinite(end) || end <= start) return 0;
    return Math.max(0, Math.min(100, (timestamp - start) * 100 / (end - start)));
  };
  const paths = (key: HistoryMetric) => {
    const result: string[] = []; let current: string[] = [];
    for (const point of points) {
      const value = point[key].avg;
      if (value === null) { if (current.length) result.push(current.join(" ")); current = []; }
      else current.push(`${xAt(point.start)},${100 - Math.max(0, Math.min(100, value * 100 / ceiling))}`);
    }
    if (current.length) result.push(current.join(" "));
    return result;
  };
  const hasSamples = values.length > 0;
  return <section className="sarmg-content-panel host-history-card" aria-label={title}><h3>{title}</h3>
    <div className="host-chart-legend">{lines.map(line => <span key={line.key}><i aria-hidden="true" style={{ background: line.color }} />{line.label}</span>)}</div>
    {!loading && failure === null && !hasSamples ? <p>{t("所选范围没有历史样本", "No historical samples in this range")}</p> : hasSamples ? <svg viewBox="0 0 100 100" role="img" aria-label={t("{0} 历史图", "{0} history chart", [title])} preserveAspectRatio="none">
      {[25, 50, 75].map(y => <line key={y} x1="0" x2="100" y1={y} y2={y} className="host-chart-gridline" vectorEffect="non-scaling-stroke" />)}
      {lines.flatMap(line => paths(line.key).map((path, index) => <polyline key={`${line.key}-${index}`} points={path} fill="none" stroke={line.color} vectorEffect="non-scaling-stroke" />))}
    </svg> : <p>{loading ? t("正在读取…", "Loading…") : t("暂时不可用", "Unavailable")}</p>}
    {hasSamples && <p className="host-chart-scale">{unit === "bytes" ? t("峰值 {0}/秒", "Peak {0}/s", [formatBytes(ceiling)]) : `${t("纵轴", "Vertical axis")} 0–${ceiling.toLocaleString()} ${unit === "percent" ? "%" : unit}`}</p>}
  </section>;
}

function LatestDevices({ report }: { report: ClientReport | null }) {
  if (report === null) return <section className="sarmg-content-panel"><h2>{t("最新设备信息", "Latest device information")}</h2><p>{t("等待首次上报", "Waiting for the first report")}</p></section>;
  const hardware = report.system.hardware;
  const groups: Array<{ title: string; records: Record<string, unknown>[]; empty: string }> = [
    ...(hardware ? [
      { title: t("硬件传感器", "Hardware sensors"), records: hardware.sensors.map(sensor => ({ id: sensor.id, label: sensor.label, [String(sensor.kind)]: sensor.value, source: sensor.source })), empty: t("未发现可读硬件传感器", "No readable hardware sensors") },
      { title: t("磁盘健康", "Disk health"), records: hardware.disk_health, empty: t("暂无 SMART 数据，请查看采集诊断", "No SMART data yet; see collection diagnostics") },
      { title: t("网络硬件", "Network hardware"), records: hardware.networks, empty: t("未发现网卡", "No network adapters") },
    ] : []),
    { title: t("网络接口", "Network interfaces"), records: report.system.networks, empty: t("未发现网络接口", "No network interfaces reported") },
    { title: t("磁盘", "Disks"), records: report.system.disks, empty: t("未发现磁盘", "No disks reported") },
    { title: t("温度传感器", "Temperature sensors"), records: report.system.temperatures, empty: t("未发现温度传感器", "No temperature sensors reported") },
    { title: t("显卡", "GPUs"), records: report.system.gpus, empty: t("未发现显卡", "No GPUs reported") },
  ];
  return <section className="sarmg-content-stack" aria-labelledby="latest-devices-heading"><div className="sarmg-content-panel"><h2 id="latest-devices-heading">{t("最新设备信息", "Latest device information")}</h2><p>{t("以下数值来自最新一份报告，采集时间：{0}", "These values come from the latest report, collected at {0}.", [formatTime(report.collected_at)])}</p></div>
    <div className="host-device-grid"><SnapshotCard title="CPU" record={{ ...report.system.cpu, ...(hardware?.cpu ?? {}) }} /><SnapshotCard title={t("内存", "RAM")} record={report.system.memory} /><SnapshotCard title={t("客户端状态", "Client health")} record={{ uptime_seconds: report.system.uptime_seconds, ...report.client }} /></div>
    {hardware && <p>{t("硬件信息采集时间：{0}；磁盘健康保留其独立采集时间。", "Hardware collected at {0}; disk health includes its own collection time.", [formatTime(hardware.collected_at)])}</p>}
    {groups.map(group => <section className="sarmg-content-panel" key={group.title}><h3>{group.title}</h3>{group.records.length ? <div className="host-device-grid">{group.records.map((record, index) => <SnapshotCard key={`${group.title}-${index}`} title={record.name ?? record.label ?? record.id ?? `${group.title} ${index + 1}`} record={record} />)}</div> : <p>{group.empty}</p>}</section>)}
    <section className="sarmg-content-panel"><h3>{t("采集能力与诊断", "Collection capabilities and diagnostics")}</h3><div className="host-device-grid">{report.capabilities.map(capability => <article className="host-device-card" key={capability.name}><h4>{displayLabel(capability.name)}</h4><dl className="host-device-details"><dt>{t("状态", "Status")}</dt><dd>{capability.available ? t("可用", "Available") : t("不可用", "Unavailable")}</dd><dt>{t("来源", "Source")}</dt><dd>{capability.source}</dd>{capability.error_kind && <><dt>{t("原因", "Reason")}</dt><dd>{displayLabel(capability.error_kind)}</dd></>}{capability.message && <><dt>{t("说明", "Details")}</dt><dd>{capability.message}</dd></>}</dl></article>)}</div></section>
  </section>;
}

function SnapshotCard({ title, record }: { title: unknown; record: Record<string, unknown> }) {
  return <article className="host-device-card"><h4>{String(title)}</h4><dl className="host-device-details">{Object.entries(record).filter(([, value]) => value !== undefined).map(([key, value]) => <Fragment key={key}><dt>{metricLabel(key)}</dt><dd>{formatMetricValue(key, value)}</dd></Fragment>)}</dl></article>;
}

const metricLabels: Record<string, readonly [string, string]> = {
  model: ["型号", "Model"], frequency_mhz: ["平均频率", "Mean frequency"], max_frequency_mhz: ["硬件最高频率", "Maximum hardware frequency"],
  per_core_frequency_mhz: ["各核心频率", "Per-core frequencies"], load_average: ["负载（1/5/15 分钟）", "Load (1/5/15 minutes)"],
  mac_address: ["MAC 地址", "MAC address"], ip_addresses: ["IP 地址", "IP addresses"], mtu: ["MTU", "MTU"], link_speed_mbps: ["链路速率", "Link speed"], operational_state: ["链路状态", "Link state"],
  fan_rpm: ["风扇转速", "Fan speed"], voltage_volts: ["电压", "Voltage"], current_amps: ["电流", "Current"], energy_joules: ["累计能量", "Energy"],
  device: ["物理设备", "Physical device"], serial_number: ["序列号", "Serial number"], protocol: ["接口协议", "Protocol"], collected_at: ["采集时间", "Collected at"],
  healthy: ["SMART 健康检查通过", "SMART health passed"], percentage_used: ["NVMe 寿命已消耗", "NVMe endurance used"], available_spare_percent: ["可用备用空间", "Available spare"], critical_warning: ["NVMe 严重警告位掩码", "NVMe critical warning bits"],
  power_on_hours: ["通电小时", "Power-on hours"], power_cycles: ["通电次数", "Power cycles"], unsafe_shutdowns: ["非安全关机", "Unsafe shutdowns"], media_errors: ["介质错误", "Media errors"], bytes_read: ["累计读取", "Total read"], bytes_written: ["累计写入", "Total written"],
  usage_percent: ["使用率", "Usage"], logical_count: ["逻辑核心", "Logical cores"], physical_count: ["物理核心", "Physical cores"], per_core_percent: ["各核心使用率", "Per-core usage"],
  total_bytes: ["总容量", "Total"], used_bytes: ["已使用", "Used"], available_bytes: ["可用", "Available"], swap_total_bytes: ["交换空间总量", "Swap total"], swap_used_bytes: ["交换空间已用", "Swap used"],
  name: ["名称", "Name"], id: ["标识", "ID"], vendor: ["厂商", "Vendor"], mount_point: ["挂载点", "Mount point"], file_system: ["文件系统", "File system"], is_read_only: ["只读", "Read only"],
  received_bytes_total: ["累计接收", "Total received"], transmitted_bytes_total: ["累计发送", "Total sent"], received_bytes_per_second: ["接收速率", "Receive rate"], transmitted_bytes_per_second: ["发送速率", "Send rate"], packets_received_total: ["接收数据包", "Packets received"], packets_transmitted_total: ["发送数据包", "Packets sent"], receive_errors_total: ["接收错误", "Receive errors"], transmit_errors_total: ["发送错误", "Transmit errors"],
  read_bytes_total: ["累计读取", "Total read"], written_bytes_total: ["累计写入", "Total written"], read_bytes_per_second: ["读取速率", "Read rate"], written_bytes_per_second: ["写入速率", "Write rate"],
  label: ["标签", "Label"], celsius: ["当前温度", "Temperature"], max_celsius: ["最高温度", "Maximum"], critical_celsius: ["临界温度", "Critical"], source: ["来源", "Source"], utilization_percent: ["使用率", "Usage"], memory_total_bytes: ["显存总量", "Memory total"], memory_used_bytes: ["显存已用", "Memory used"], temperature_celsius: ["温度", "Temperature"], power_watts: ["功耗", "Power"], core_clock_mhz: ["核心频率", "Core clock"], memory_clock_mhz: ["显存频率", "Memory clock"], pcie_rx_bytes_per_second: ["PCIe 接收速率", "PCIe receive rate"], pcie_tx_bytes_per_second: ["PCIe 发送速率", "PCIe transmit rate"],
  uptime_seconds: ["运行时间", "Uptime"], spool_pending_batches: ["待发送批次", "Pending batches"], collector_errors: ["采集错误", "Collection errors"],
};
function metricLabel(key: string) { const label = metricLabels[key]; return label ? t(...label) : key.replaceAll("_", " "); }
function formatMetricValue(key: string, value: unknown): string {
  if (value === null) return t("不可用", "Unavailable");
  if (Array.isArray(value)) return value.length ? value.map(item => item === null ? t("不可用", "Unavailable") : typeof item === "number" ? `${item.toFixed(1)}${key.includes("percent") ? "%" : key.endsWith("_mhz") ? " MHz" : ""}` : String(item)).join(" · ") : t("无", "None");
  if (key === "collected_at" && typeof value === "string") return formatTime(value);
  const unit = ({fan_rpm:"RPM",voltage_volts:"V",current_amps:"A",energy_joules:"J",link_speed_mbps:"Mbps"} as Record<string,string>)[key];
  if (unit && typeof value === "number") return `${value.toLocaleString()} ${unit}`;
  if (typeof value === "boolean") return value ? t("是", "Yes") : t("否", "No");
  if (key === "uptime_seconds" && (typeof value === "number" || typeof value === "string")) return formatDuration(Number(value));
  if (key.includes("bytes")) return `${formatBytes(Number(value))}${key.endsWith("per_second") ? t("/秒", "/s") : ""}`;
  if (key.includes("percent") && typeof value === "number") return `${value.toFixed(1)}%`;
  if (key.includes("celsius") && typeof value === "number") return `${value.toFixed(1)} ℃`;
  if (key.endsWith("_watts") && typeof value === "number") return `${value.toFixed(1)} W`;
  if (key.endsWith("_mhz") && typeof value === "number") return `${value.toLocaleString()} MHz`;
  return typeof value === "number" ? value.toLocaleString() : String(value);
}
function formatBytes(value: number): string {
  if (!Number.isFinite(value)) return t("不可用", "Unavailable");
  const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"]; let amount = Math.max(0, value); let unit = 0;
  while (amount >= 1024 && unit < units.length - 1) { amount /= 1024; unit += 1; }
  return `${amount >= 100 || unit === 0 ? amount.toFixed(0) : amount.toFixed(1)} ${units[unit]}`;
}
function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds)) return t("不可用", "Unavailable");
  const days = Math.floor(seconds / 86400); const hours = Math.floor(seconds % 86400 / 3600); const minutes = Math.floor(seconds % 3600 / 60);
  return days ? t("{0} 天 {1} 小时", "{0}d {1}h", [String(days), String(hours)]) : t("{0} 小时 {1} 分钟", "{0}h {1}m", [String(hours), String(minutes)]);
}

function formatPercent(value: number | null): string { return value === null ? t("不可用", "Unavailable") : `${value.toFixed(1)}%`; }
function formatTime(value: string | null): string { return value === null ? t("尚未上报", "Not yet reported") : new Date(value).toLocaleString(getLocale()); }
