import { displayLabel } from "./display-labels";
import { t, getLocale } from "@xcss/web/admin-ui/i18n";
import { Fragment, useEffect, useRef, useState, type ReactNode } from "react";
import { Button, ConfirmDangerDialog, ErrorState, LoadingState } from "@xcss/web/admin-ui";
import { errorRequestId, useAdminApplication } from "@xcss/web/admin-shell";
import {
  isHistorySeriesResponse,
  isHostDetailResponse,
  isNoContent,
  type ClientReport,
  type HistorySeriesResponse,
  type HostDetailResponse,
} from "./api";

type Failure = { requestId?: string };

export function HostDetails({ hostId, hostName, refreshSignal, removed, overview }: { hostId: string; hostName: string; refreshSignal: number; removed(): void; overview: ReactNode }) {
  const { client, notify } = useAdminApplication();
  const [detail, setDetail] = useState<HostDetailResponse | null>(null);
  const [history, setHistory] = useState<HistorySeriesResponse | null>(null);
  const [historyHours, setHistoryHours] = useState(1);
  const [historyLoading, setHistoryLoading] = useState(true);
  const [historyFailure, setHistoryFailure] = useState<Failure | null>(null);
  const [historyGeneration, setHistoryGeneration] = useState(0);
  const [updatedAt, setUpdatedAt] = useState<Date | null>(null);
  const [failure, setFailure] = useState<Failure | null>(null);
  const [pending, setPending] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const mutation = useRef<AbortController | null>(null);
  const appliedRefreshSignal = useRef(refreshSignal);
  const requestRefresh = useRef<() => void>(() => undefined);
  const appliedHistoryRefresh = useRef({ refreshSignal, historyGeneration });
  const historySelection = useRef<string | null>(null);
  const requestHistoryRefresh = useRef<() => void>(() => undefined);

  useEffect(() => {
    let timer: number | undefined;
    let controller: AbortController | undefined;
    let stopped = false;
    let inFlight = false;
    let refreshQueued = false;
    let forceQueued = false;
    async function refresh(force = false) {
      if (stopped || (!force && document.hidden)) return;
      if (inFlight) { refreshQueued = true; forceQueued ||= force; return; }
      inFlight = true;
      refreshQueued = false;
      forceQueued = false;
      controller = new AbortController();
      try {
        const value = await client.request(`/api/v1/monitoring/hosts/${hostId}`, isHostDetailResponse, { signal: controller.signal });
        if (stopped || controller.signal.aborted || value.host.id !== hostId) return;
        setDetail(value); setUpdatedAt(new Date()); setFailure(null);
      } catch (error) {
        if (!stopped && !controller.signal.aborted) setFailure({ requestId: errorRequestId(error) });
      } finally {
        inFlight = false;
        if (stopped) return;
        if (refreshQueued) void refresh(forceQueued);
        else if (!document.hidden) timer = window.setTimeout(refresh, 2_000);
      }
    }
    const visible = () => {
      if (timer !== undefined) { clearTimeout(timer); timer = undefined; }
      if (document.hidden) return;
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
  }, [client, hostId]);

  useEffect(() => {
    if (appliedRefreshSignal.current === refreshSignal) return;
    appliedRefreshSignal.current = refreshSignal;
    requestRefresh.current();
  }, [refreshSignal]);

  useEffect(() => {
    let timer: number | undefined;
    let controller: AbortController | undefined;
    let stopped = false;
    let inFlight = false;
    let refreshQueued = false;
    let forceQueued = false;
    const selection = `${hostId}/${historyHours}`;
    const selectionChanged = historySelection.current !== selection;
    const explicitSelection = historySelection.current !== null && selectionChanged;
    historySelection.current = selection;
    if (selectionChanged) {
      setHistory(null); setHistoryLoading(true); setHistoryFailure(null);
    }
    async function refreshHistory(force = false) {
      if (stopped || (!force && document.hidden)) return;
      if (inFlight) { refreshQueued = true; forceQueued ||= force; return; }
      inFlight = true;
      refreshQueued = false;
      forceQueued = false;
      controller = new AbortController();
      const from = new Date(Date.now() - historyHours * 60 * 60 * 1000).toISOString();
      try {
        const value = await client.request(`/api/v1/monitoring/hosts/${hostId}/history?from=${encodeURIComponent(from)}&resolution=auto&max_points=720`, isHistorySeriesResponse, { signal: controller.signal });
        if (!stopped && !controller.signal.aborted && value.host_id === hostId) {
          setHistory(value); setHistoryFailure(null);
        }
      } catch (error) {
        if (!stopped && !controller.signal.aborted) setHistoryFailure({ requestId: errorRequestId(error) });
      } finally {
        inFlight = false;
        if (!stopped && !controller.signal.aborted) {
          setHistoryLoading(false);
          if (refreshQueued) void refreshHistory(forceQueued);
          else if (!document.hidden) timer = window.setTimeout(refreshHistory, historyHours <= 1 ? 5_000 : 30_000);
        }
      }
    }
    const visible = () => {
      if (timer !== undefined) { clearTimeout(timer); timer = undefined; }
      if (!document.hidden) void refreshHistory();
    };
    requestHistoryRefresh.current = () => {
      if (timer !== undefined) { clearTimeout(timer); timer = undefined; }
      void refreshHistory(true);
    };
    document.addEventListener("visibilitychange", visible);
    void refreshHistory(explicitSelection);
    return () => { stopped = true; requestHistoryRefresh.current = () => undefined; controller?.abort(); if (timer !== undefined) clearTimeout(timer); document.removeEventListener("visibilitychange", visible); };
  }, [client, hostId, historyHours]);

  useEffect(() => {
    const applied = appliedHistoryRefresh.current;
    if (applied.refreshSignal === refreshSignal && applied.historyGeneration === historyGeneration) return;
    appliedHistoryRefresh.current = { refreshSignal, historyGeneration };
    requestHistoryRefresh.current();
  }, [refreshSignal, historyGeneration]);

  useEffect(() => () => mutation.current?.abort(), []);
  async function remove() {
    if (mutation.current) return;
    const controller = new AbortController(); mutation.current = controller; setPending(true); setFailure(null);
    try {
      await client.request(`/api/v1/monitoring/managed-instances/${hostId}`, isNoContent, { method: "DELETE", signal: controller.signal });
      if (!controller.signal.aborted) {
        setDeleting(false);
        notify(t("实例已移除", "Instance removed"));
        removed();
      }
    } catch (error) { if (!controller.signal.aborted) setFailure({ requestId: errorRequestId(error) }); }
    finally { if (!controller.signal.aborted) { mutation.current = null; setPending(false); } }
  }
  // Keep the paired workspace mounted while data loads so requests cannot replace the selected category.
  const currentDetail = detail?.host.id === hostId ? detail : null;
  const host = currentDetail?.host;
  const latest = currentDetail?.latest ?? null;
  const metricLagMilliseconds = host && latest ? Date.parse(host.last_seen_at) - Date.parse(latest.collected_at) : 0;
  const metricsLagging = host?.status === "online" && latest !== null
    && metricLagMilliseconds > Math.max(60_000, latest.interval_seconds * 2_000);
  const metricsAhead = host?.status === "online" && latest !== null && metricLagMilliseconds < -60_000;
  const monitoringStatus = host === undefined ? failure === null ? <LoadingState>{t("正在读取实例详情…", "Loading instance details…")}</LoadingState> : null
    : <div className="xcss-content-stack xsos-status"><h4>{host.name}</h4><dl className="host-detail-list">
      <dt>{t("状态", "Status")}</dt><dd>{displayLabel(host.status)}</dd>
      <dt>{t("系统", "System")}</dt><dd>{host.os}{host.os_version ? ` ${host.os_version}` : ""} / {host.arch}</dd>
      <dt>{t("内核版本", "Kernel version")}</dt><dd>{host.kernel_version ?? t("不可用", "Unavailable")}</dd>
      <dt>{t("客户端版本", "Client version")}</dt><dd>{host.client_version}</dd>
      <dt>{t("最新 CPU", "Latest CPU")}</dt><dd>{formatPercent(host.cpu_usage_percent)}</dd>
      <dt>{t("最新内存", "Latest memory")}</dt><dd>{formatPercent(host.memory_usage_percent)}</dd>
      <dt>{t("数据采集时间", "Data collected at")}</dt><dd>{formatTime(host.latest_collected_at)}</dd>
      <dt>{t("服务端最近收到", "Last received by server")}</dt><dd>{formatTime(latest === null ? null : host.last_seen_at)}</dd>
      <dt>{t("页面最近更新", "Page last updated")}</dt><dd>{updatedAt?.toLocaleString(getLocale()) ?? "—"}</dd>
    </dl>{latest && <SnapshotCard title={t("客户端状态", "Client health")} record={{ uptime_seconds: latest.system.uptime_seconds, ...latest.client }} framed={false} />}{metricsLagging && <p role="status">{t("服务端仍在收到上报，但当前展示的指标采集时间明显较早；客户端可能正在补传或已调整时钟。", "The server is still receiving reports, but the displayed metrics were collected much earlier. The client may be replaying queued reports or may have adjusted its clock.")}</p>}
      {metricsAhead && <p role="status">{t("指标采集时间明显晚于服务端接收时间；客户端时钟可能偏快，后续指标可能暂时不更新。", "Metric collection time is well ahead of server receipt time. The client clock may be fast, and subsequent metrics may not update until it catches up.")}</p>}
    </div>;
  const instanceOverview = <section className="host-overview">{overview}<section className="host-overview-section host-instance-actions" aria-label={t("实例操作", "Instance actions")}><div className="xcss-actions"><Button disabled={pending} onClick={() => setDeleting(true)}>{t("删除实例", "Delete instance")}</Button></div></section></section>;
  return <div className="xcss-content-stack">
    {currentDetail !== null && failure && <ErrorState requestId={failure.requestId}>{t("自动更新暂时失败，页面保留上次成功数据。", "Automatic update failed temporarily. The last successful data is retained.")}</ErrorState>}
    <LatestDevices report={latest} detailsPending={currentDetail === null} instanceOverview={instanceOverview} monitoringStatus={monitoringStatus} response={history} hours={historyHours} loading={historyLoading} failure={historyFailure} retry={() => setHistoryGeneration(value => value + 1)} changeHours={setHistoryHours} />
    {currentDetail === null && failure && <ErrorState requestId={failure.requestId}>{t("无法读取实例详情", "Unable to load instance details")}</ErrorState>}
    {deleting && <ConfirmDangerDialog title={t("删除监控实例", "Delete monitoring instance")} description={t("移除 {0} 的监控数据和绑定凭据。该客户端需要重新配对才能再次接入。", "Remove monitoring data and bound credentials for {0}. The client must pair again to reconnect.", [host?.name ?? hostName])}
      pending={pending} onClose={() => { if (!mutation.current) setDeleting(false); }} onConfirm={() => void remove()} />}
  </div>;
}

type HistoryMetric = keyof Pick<HistorySeriesResponse["points"][number],
  "cpu_usage_percent" | "memory_usage_percent" | "disk_read_bytes_per_second" | "disk_written_bytes_per_second" | "max_temperature_celsius" | "gpu_utilization_percent" | "gpu_memory_usage_percent" | "cpu_frequency_mhz" | "gpu_power_watts" | "gpu_core_clock_mhz" | "max_fan_rpm" | "max_disk_temperature_celsius" | "max_disk_percentage_used">;
type ChartLine = { key: HistoryMetric; label: string; color: string };
type HistoryChartProps = { response: HistorySeriesResponse | null; hours: number; loading: boolean; failure: Failure | null; retry(): void; changeHours(value: number): void };

function HistoryChart({ response, hours, loading, failure, retry, changeHours }: HistoryChartProps) {
  const charts: Array<{ title: string; unit: "percent" | "bytes" | "MHz" | "W" | "RPM" | "℃"; lines: ChartLine[] }> = [
    { title: "CPU", unit: "percent", lines: [
      { key: "cpu_usage_percent", label: t("使用率", "Usage"), color: "#2878b5" },
    ] },
    { title: "GPU", unit: "percent", lines: [
      { key: "gpu_utilization_percent", label: t("使用率", "Usage"), color: "#6f58a8" },
      { key: "gpu_memory_usage_percent", label: t("显存使用率", "Memory usage"), color: "#bd6eaa" },
    ] },
    { title: t("磁盘 I/O", "Disk I/O"), unit: "bytes", lines: [
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
  return <div className="xcss-content-stack host-history-content"><div className="host-history-toolbar"><p>{response ? t("每点 {0} 秒，来源：{1}；横轴按采样时间，缺失区间不连线。", "{0} seconds per point, source: {1}; the horizontal axis uses sample time and missing intervals are not connected.", [String(response.step_seconds), response.source]) : loading ? t("正在读取历史趋势…", "Loading history trends…") : null}</p>
    <div className="xcss-actions">{[[0.25, "15m"], [1, "1h"], [6, "6h"], [24, "24h"], [168, "7d"], [720, "30d"]].map(([value, label]) => <Button key={label} aria-pressed={hours === value} onClick={() => changeHours(Number(value))}>{label}</Button>)}</div>
    {failure && <ErrorState requestId={failure.requestId} onRetry={retry}>{t("无法读取所选范围的历史趋势。", "Unable to load history trends for the selected range.")}</ErrorState>}</div>
    <div className="host-history-grid">{charts.map(chart => <MetricChartBlock key={chart.title} title={chart.title} unit={chart.unit} response={response} lines={chart.lines} loading={loading} failure={failure} />)}</div>
  </div>;
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
  const floor = unit === "℃" ? Math.min(0, ...values) : 0;
  const xAt = (value: string) => {
    const timestamp = Date.parse(value);
    if (!Number.isFinite(timestamp) || !Number.isFinite(start) || !Number.isFinite(end) || end <= start) return 0;
    return Math.max(0, Math.min(100, (timestamp - start) * 100 / (end - start)));
  };
  const stepMilliseconds = (response?.step_seconds ?? 0) * 1000;
  const segments = (key: HistoryMetric) => {
    type ChartPoint = { x: number; y: number };
    const result: ChartPoint[][] = []; let current: ChartPoint[] = [];
    let previousStart: number | null = null;
    for (const point of points) {
      const value = point[key].avg;
      const start = Date.parse(point.start);
      if (current.length && (value === null || previousStart === null || start > previousStart + stepMilliseconds)) {
        result.push(current); current = [];
      }
      if (value !== null) current.push({ x: xAt(point.start), y: 100 - Math.max(0, Math.min(100, (value - floor) * 100 / (ceiling - floor))) });
      previousStart = start;
    }
    if (current.length) result.push(current);
    return result;
  };
  const hasSamples = values.length > 0;
  return <section className="xcss-content-panel host-history-card" aria-label={title}><h3>{title}</h3>
    <div className="host-chart-legend">{lines.map(line => <span key={line.key}><i aria-hidden="true" style={{ background: line.color }} />{line.label}</span>)}</div>
    {!loading && failure === null && !hasSamples ? <p>{t("所选范围没有历史样本", "No historical samples in this range")}</p> : hasSamples ? <svg viewBox="0 0 100 100" role="img" aria-label={t("{0} 历史图", "{0} history chart", [title])} preserveAspectRatio="none">
      {[25, 50, 75].map(y => <line key={y} x1="0" x2="100" y1={y} y2={y} className="host-chart-gridline" vectorEffect="non-scaling-stroke" />)}
      {lines.flatMap(line => segments(line.key).map((segment, index) => segment.length === 1
        ? <circle key={`${line.key}-${index}`} cx={segment[0].x} cy={segment[0].y} r="0.8" fill={line.color} />
        : <polyline key={`${line.key}-${index}`} points={segment.map(({ x, y }) => `${x},${y}`).join(" ")} fill="none" stroke={line.color} vectorEffect="non-scaling-stroke" />))}
    </svg> : <p>{loading ? t("正在读取…", "Loading…") : t("暂时不可用", "Unavailable")}</p>}
    {hasSamples && <p className="host-chart-scale">{unit === "bytes" ? t("峰值 {0}/秒", "Peak {0}/s", [formatBytes(ceiling)]) : `${t("纵轴", "Vertical axis")} ${floor.toLocaleString(getLocale())}–${ceiling.toLocaleString(getLocale())} ${unit === "percent" ? "%" : unit}`}</p>}
  </section>;
}

type InformationCategory = { id: string; label: string; content: ReactNode };

function LatestDevices({ report, detailsPending, instanceOverview, monitoringStatus, ...historyProps }: { report: ClientReport | null; detailsPending: boolean; instanceOverview: ReactNode; monitoringStatus: ReactNode } & HistoryChartProps) {
  const [category, setCategory] = useState("history");
  const waitingForDevices = detailsPending ? t("正在读取实例详情…", "Loading instance details…") : t("等待首次上报", "Waiting for the first report");
  const categories: InformationCategory[] = [
    { id: "history", label: t("历史趋势", "History trends"), content: <HistoryChart {...historyProps} /> },
    { id: "instance", label: t("实例概览", "Instance overview"), content: instanceOverview },
    ...(report === null ? [
      { id: "cpu", label: "CPU", content: <p className="host-device-card host-device-empty">{waitingForDevices}</p> },
      { id: "memory", label: t("内存", "RAM"), content: <p className="host-device-card host-device-empty">{waitingForDevices}</p> },
    ] : deviceCategories(report)),
    { id: "monitoring", label: t("监控状态", "Monitoring status"), content: monitoringStatus },
  ];
  const selected = categories.find(item => item.id === category) ?? categories[0];
  const contained = selected.id === "instance" || selected.id === "monitoring";
  return <div className="xcss-content-stack host-details-layout">
    <nav className="host-device-navigation xcss-secondary-navigation" aria-label={t("设备信息分类", "Device information categories")}>{categories.map(item => <Button key={item.id} type="button" aria-pressed={selected.id === item.id} aria-controls="host-device-content" onClick={() => setCategory(item.id)}>{item.label}</Button>)}</nav>
    <section className={`host-details-panel${contained ? " xcss-content-panel host-details-panel--contained" : ""}`} aria-label={t("实例详情", "Instance details")}><section id="host-device-content" aria-label={selected.label}>{selected.content}</section></section>
  </div>;
}

function deviceCategories(report: ClientReport): InformationCategory[] {
  const hardware = report.system.hardware;
  const { interfaces, unmatchedHardware } = networkDisplayRecords(report.system.networks, hardware?.networks ?? []);
  const groups: Array<{ id: string; title: string; records: Record<string, unknown>[]; empty: string }> = [
    ...([
      ["thunderbolt", t("雷电 / USB4", "Thunderbolt / USB4")],
      ["monitor", t("显示器", "Monitors")],
      ["bluetooth", t("蓝牙", "Bluetooth")],
      ["usb_controller", t("USB 控制器", "USB controllers")],
      ["usb_device", t("USB 设备", "USB devices")],
      ["audio", t("音频设备", "Audio devices")],
    ] as const).map(([kind, title]) => ({ id: kind, title, records: (hardware?.devices ?? []).filter(device => device.kind === kind).map(({ kind: _kind, ...device }) => ({ ...device, ...(hardware?.inventory_collected_at ? { collected_at: hardware.inventory_collected_at } : {}) })), empty: t("当前报告未识别到此类设备，请查看采集诊断。", "No devices of this type identified in the current report; see collection diagnostics.") })),
    ...(hardware ? [
      { id: "sensors", title: t("硬件传感器", "Hardware sensors"), records: hardware.sensors.map(sensor => ({ id: sensor.id, label: sensor.label, [String(sensor.kind)]: sensor.value, source: sensor.source })), empty: t("未发现可读硬件传感器", "No readable hardware sensors") },
      { id: "disk-health", title: t("磁盘健康", "Disk health"), records: hardware.disk_health, empty: t("暂无 SMART 数据，请查看采集诊断", "No SMART data yet; see collection diagnostics") },
    ] : []),
    { id: "networks", title: t("网络接口", "Network interfaces"), records: [...interfaces, ...unmatchedHardware], empty: t("未发现网络接口", "No network interfaces reported") },
    { id: "disks", title: t("磁盘", "Disks"), records: report.system.disks.map(diskDisplayRecord), empty: t("未发现磁盘", "No disks reported") },
    { id: "temperatures", title: t("温度传感器", "Temperature sensors"), records: report.system.temperatures, empty: t("未发现温度传感器", "No temperature sensors reported") },
    { id: "gpus", title: t("显卡", "GPUs"), records: gpuDisplayRecords(report.system.gpus), empty: t("未发现显卡", "No GPUs reported") },
  ];
  return [
    { id: "cpu", label: "CPU", content: <div className="host-device-grid"><SnapshotCard title="CPU" showTitle={false} record={{ ...report.system.cpu, ...(hardware?.cpu ?? {}) }} /></div> },
    { id: "memory", label: t("内存", "RAM"), content: <><div className="host-device-grid"><SnapshotCard title={t("内存", "RAM")} showTitle={false} record={report.system.memory} /></div>{hardware?.memory_modules?.length ? <div className="host-device-grid">{hardware.memory_modules.map((module, index) => <SnapshotCard key={String(module.source) + ":" + String(module.id)} title={module.locator ?? module.model ?? t("内存模块 {0}", "Memory module {0}", [String(index + 1)])} record={{ ...module, ...(hardware.inventory_collected_at ? { collected_at: hardware.inventory_collected_at } : {}) }} />)}</div> : <p className="host-device-empty">{t("当前报告未提供内存模块规格，请查看采集诊断。", "No memory module specifications in the current report; see collection diagnostics.")}</p>}</> },
    { id: "physical-networks", label: t("网络硬件", "Network hardware"), content: hardware?.physical_networks?.length ? <div className="host-device-grid">{hardware.physical_networks.map(adapter => <SnapshotCard key={String(adapter.source) + ":" + String(adapter.id)} title={adapter.name ?? adapter.id} record={adapter} />)}</div> : <p className="host-device-card host-device-empty">{t("当前报告未提供可确认的物理网卡信息。", "The current report does not provide verified physical network adapter information.")}</p> },
    ...groups.map(group => ({ id: group.id, label: group.title, content: group.records.length ? <div className="host-device-grid">{group.records.map((record, recordIndex) => <SnapshotCard key={`${group.id}-${recordIndex}`} title={record.name ?? record.label ?? record.id ?? `${group.title} ${recordIndex + 1}`} record={record} />)}</div> : <p className="host-device-card host-device-empty">{group.empty}</p> })),
    { id: "diagnostics", label: t("采集能力与诊断", "Collection capabilities and diagnostics"), content: report.capabilities.length ? <div className="host-device-grid">{report.capabilities.map(capability => <article className="host-device-card" key={capability.name}><h4>{displayLabel(capability.name)}</h4><dl className="host-device-details"><dt>{t("状态", "Status")}</dt><dd>{capability.available ? t("可用", "Available") : t("不可用", "Unavailable")}</dd><dt>{t("来源", "Source")}</dt><dd>{capability.source}</dd>{capability.error_kind && <><dt>{t("原因", "Reason")}</dt><dd>{displayLabel(capability.error_kind)}</dd></>}{capability.message && <><dt>{t("说明", "Details")}</dt><dd>{capability.message}</dd></>}</dl></article>)}</div> : <p className="host-device-card host-device-empty">{t("当前报告没有采集能力信息。", "The current report has no collection capability information.")}</p> },
  ];
}

function networkDisplayRecords(interfaces: Record<string, unknown>[], hardware: Record<string, unknown>[]) {
  const interfaceCounts = new Map<string, number>();
  const hardwareCounts = new Map<string, number>();
  for (const record of interfaces) if (typeof record.name === "string") interfaceCounts.set(record.name, (interfaceCounts.get(record.name) ?? 0) + 1);
  for (const record of hardware) if (typeof record.name === "string") hardwareCounts.set(record.name, (hardwareCounts.get(record.name) ?? 0) + 1);
  const uniqueHardware = new Map(hardware.filter(record => typeof record.name === "string" && hardwareCounts.get(record.name) === 1).map(record => [record.name, record]));
  const merged = new Set<string>();
  const display = interfaces.map(record => {
    if (typeof record.name !== "string" || interfaceCounts.get(record.name) !== 1) return record;
    const extra = uniqueHardware.get(record.name);
    if (!extra) return record;
    merged.add(record.name);
    return { ...record, ...extra };
  });
  return { interfaces: display, unmatchedHardware: hardware.filter(record => typeof record.name !== "string" || !merged.has(record.name)) };
}

function gpuDisplayRecords(gpus: Record<string, unknown>[]): Record<string, unknown>[] {
  const display = gpus.map(gpu => ({ ...gpu }));
  const hidden = new Set<number>();
  for (const [index, fallback] of gpus.entries()) {
    if (fallback.vendor !== "amd" || fallback.source !== "amd-adlx-no-luid" || typeof fallback.name !== "string") continue;
    if (gpus.filter(gpu => gpu.vendor === "amd" && gpu.name === fallback.name && gpu.source === "amd-adlx-no-luid").length !== 1) continue;
    const matches = gpus.flatMap((gpu, candidate) =>
      gpu.vendor === "amd" && gpu.name === fallback.name && gpu.source === "windows-dxgi-pdh" ? [candidate] : []);
    if (matches.length === 0 || matches.length > 2) continue;
    const active = matches.filter(candidate =>
      typeof gpus[candidate].utilization_percent === "number" || typeof gpus[candidate].memory_used_bytes === "number");
    if (active.length !== 1 || matches.some(candidate => candidate !== active[0]
      && (typeof gpus[candidate].utilization_percent === "number" || typeof gpus[candidate].memory_used_bytes === "number"
        || gpus[candidate].memory_total_bytes !== gpus[active[0]].memory_total_bytes))) continue;
    // Older ADLX drivers have no LUID. Group one active DXGI reading with
    // its static-only alias, while keeping all source IDs visible.
    const primary = display[active[0]];
    for (const key of ["temperature_celsius", "power_watts", "core_clock_mhz", "memory_clock_mhz"])
      if (primary[key] == null && typeof fallback[key] === "number") primary[key] = fallback[key];
    primary.related_ids = [...matches.filter(candidate => candidate !== active[0]).map(candidate => gpus[candidate].id), fallback.id];
    primary.source = "windows-dxgi-pdh + amd-adlx-no-luid";
    hidden.add(index);
    for (const candidate of matches) if (candidate !== active[0]) hidden.add(candidate);
  }
  return display.filter((_, index) => !hidden.has(index));
}

function diskDisplayRecord(record: Record<string, unknown>): Record<string, unknown> {
  const total = parseJsonU64(record.total_bytes);
  const available = parseJsonU64(record.available_bytes);
  if (total === null || total === 0n || available === null || available > total) {
    return record;
  }
  const used = total - available;
  return {
    ...record,
    used_bytes: used <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(used) : used.toString(),
    usage_percent: Number(used) * 100 / Number(total),
  };
}

// The report contract encodes u64 values beyond JavaScript's exact range as decimal strings.
function parseJsonU64(value: unknown): bigint | null {
  if (typeof value === "number") return Number.isSafeInteger(value) && value >= 0 ? BigInt(value) : null;
  if (typeof value !== "string" || !/^(?:0|[1-9][0-9]*)$/.test(value)) return null;
  const parsed = BigInt(value);
  return parsed <= 18_446_744_073_709_551_615n ? parsed : null;
}

function SnapshotCard({ title, record, showTitle = true, framed = true }: { title: unknown; record: Record<string, unknown>; showTitle?: boolean; framed?: boolean }) {
  return <article className={`host-device-card${framed ? "" : " host-device-card--plain"}`}>{showTitle && <h4>{String(title)}</h4>}<dl className="host-device-details">{Object.entries(record).filter(([, value]) => value !== undefined).map(([key, value]) => <Fragment key={key}><dt>{metricLabel(key)}</dt><dd>{formatMetricValue(key, value)}</dd></Fragment>)}</dl></article>;
}

const metricLabels: Record<string, readonly [string, string]> = {
  module_version: ["模块 / 固件版本", "Module / firmware version"],
  locator: ["插槽 / 位置", "Slot / location"], memory_type: ["内存代际", "Memory generation"], form_factor: ["封装类型", "Form factor"], capacity_bytes: ["模块容量", "Module capacity"],
  speed_mt_s: ["标称传输速率", "Rated transfer rate"], configured_speed_mt_s: ["配置传输速率", "Configured transfer rate"], reported_speed: ["系统报告速率", "Reported rate"],
  vendor_id: ["厂商 ID", "Vendor ID"], product_id: ["产品 ID", "Product ID"], revision: ["设备 / 固件修订", "Device / firmware revision"], version: ["协议版本", "Protocol version"], bus: ["总线 / 传输方式", "Bus / transport"], driver: ["驱动", "Driver"], connection: ["连接接口", "Connection"], speed_mbps: ["设备链路速率", "Device link speed"],
  model: ["型号", "Model"], frequency_mhz: ["平均频率", "Mean frequency"], max_frequency_mhz: ["硬件最高频率", "Maximum hardware frequency"],
  per_core_frequency_mhz: ["各核心频率", "Per-core frequencies"], load_average: ["负载（1/5/15 分钟）", "Load (1/5/15 minutes)"],
  mac_address: ["MAC 地址", "MAC address"], ip_addresses: ["IP 地址", "IP addresses"], interface_name: ["接口名称", "Interface name"], mtu: ["MTU", "MTU"], link_speed_mbps: ["链路速率", "Link speed"], operational_state: ["链路状态", "Link state"],
  fan_rpm: ["风扇转速", "Fan speed"], voltage_volts: ["电压", "Voltage"], current_amps: ["电流", "Current"], energy_joules: ["累计能量", "Energy"],
  device: ["物理设备", "Physical device"], serial_number: ["序列号", "Serial number"], protocol: ["接口协议", "Protocol"], collected_at: ["采集时间", "Collected at"],
  healthy: ["SMART 健康检查通过", "SMART health passed"], percentage_used: ["NVMe 寿命已消耗", "NVMe endurance used"], available_spare_percent: ["可用备用空间", "Available spare"], critical_warning: ["NVMe 严重警告位掩码", "NVMe critical warning bits"],
  power_on_hours: ["通电小时", "Power-on hours"], power_cycles: ["通电次数", "Power cycles"], unsafe_shutdowns: ["非安全关机", "Unsafe shutdowns"], media_errors: ["介质错误", "Media errors"], bytes_read: ["累计读取", "Total read"], bytes_written: ["累计写入", "Total written"],
  usage_percent: ["使用率", "Usage"], logical_count: ["逻辑核心", "Logical cores"], physical_count: ["物理核心", "Physical cores"], per_core_percent: ["各核心使用率", "Per-core usage"],
  total_bytes: ["总容量", "Total"], used_bytes: ["已使用", "Used"], available_bytes: ["可用", "Available"], swap_total_bytes: ["交换空间总量", "Swap total"], swap_used_bytes: ["交换空间已用", "Swap used"],
  name: ["名称", "Name"], id: ["标识", "ID"], related_ids: ["相关读数标识", "Related reading IDs"], vendor: ["厂商", "Vendor"], mount_point: ["挂载点", "Mount point"], file_system: ["文件系统", "File system"], is_read_only: ["只读", "Read only"],
  received_bytes_total: ["累计接收", "Total received"], transmitted_bytes_total: ["累计发送", "Total sent"], received_bytes_per_second: ["接收速率", "Receive rate"], transmitted_bytes_per_second: ["发送速率", "Send rate"], packets_received_total: ["接收数据包", "Packets received"], packets_transmitted_total: ["发送数据包", "Packets sent"], receive_errors_total: ["接收错误", "Receive errors"], transmit_errors_total: ["发送错误", "Transmit errors"],
  read_bytes_total: ["累计读取", "Total read"], written_bytes_total: ["累计写入", "Total written"], read_bytes_per_second: ["读取速率", "Read rate"], written_bytes_per_second: ["写入速率", "Write rate"],
  label: ["标签", "Label"], celsius: ["当前温度", "Temperature"], max_celsius: ["最高温度", "Maximum"], critical_celsius: ["临界温度", "Critical"], source: ["来源", "Source"], utilization_percent: ["使用率", "Usage"], memory_total_bytes: ["显存总量", "Memory total"], memory_used_bytes: ["显存已用", "Memory used"], temperature_celsius: ["温度", "Temperature"], power_watts: ["功耗", "Power"], core_clock_mhz: ["核心频率", "Core clock"], memory_clock_mhz: ["显存频率", "Memory clock"], pcie_rx_bytes_per_second: ["PCIe 接收速率", "PCIe receive rate"], pcie_tx_bytes_per_second: ["PCIe 发送速率", "PCIe transmit rate"],
  uptime_seconds: ["运行时间", "Uptime"], spool_pending_batches: ["待发送批次", "Pending batches"], collector_errors: ["采集错误", "Collection errors"],
};
function metricLabel(key: string) { const label = Object.hasOwn(metricLabels, key) ? metricLabels[key] : undefined; return label ? t(...label) : key.replaceAll("_", " "); }
function formatMetricValue(key: string, value: unknown): string {
  if (value === null) return t("不可用", "Unavailable");
  if (Array.isArray(value)) return value.length ? value.map(item => item === null ? t("不可用", "Unavailable") : typeof item === "number" ? `${item.toFixed(1)}${key.includes("percent") ? "%" : key.endsWith("_mhz") ? " MHz" : ""}` : String(item)).join(" · ") : t("无", "None");
  if (key === "collected_at" && typeof value === "string") return formatTime(value);
  const unit = ({fan_rpm:"RPM",voltage_volts:"V",current_amps:"A",energy_joules:"J",link_speed_mbps:"Mbps",speed_mbps:"Mbps",speed_mt_s:"MT/s",configured_speed_mt_s:"MT/s"} as Record<string,string>)[key];
  if (unit && typeof value === "number") return `${value.toLocaleString(getLocale())} ${unit}`;
  if (typeof value === "boolean") return value ? t("是", "Yes") : t("否", "No");
  if (key === "uptime_seconds") {
    const seconds = parseJsonU64(value);
    return seconds === null ? t("不可用", "Unavailable") : formatDuration(seconds);
  }
  if (key.includes("bytes")) {
    const bytes = typeof value === "string" ? parseJsonU64(value) : null;
    return `${formatBytes(bytes ?? Number(value))}${key.endsWith("per_second") ? t("/秒", "/s") : ""}`;
  }
  if (key.includes("percent") && typeof value === "number") return `${value.toFixed(1)}%`;
  if (key.includes("celsius") && typeof value === "number") return `${value.toFixed(1)} ℃`;
  if (key.endsWith("_watts") && typeof value === "number") return `${value.toFixed(1)} W`;
  if (key.endsWith("_mhz") && typeof value === "number") return `${value.toLocaleString(getLocale())} MHz`;
  return typeof value === "number" ? value.toLocaleString(getLocale()) : String(value);
}
function formatBytes(value: number | bigint): string {
  if (typeof value === "bigint") {
    const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let divisor = 1n; let unit = 0;
    while (value >= divisor * 1024n && unit < units.length - 1) { divisor *= 1024n; unit += 1; }
    if (unit === 0 || value >= divisor * 100n) return `${(value + divisor / 2n) / divisor} ${units[unit]}`;
    const tenths = (value * 10n + divisor / 2n) / divisor;
    return `${tenths / 10n}.${tenths % 10n} ${units[unit]}`;
  }
  if (!Number.isFinite(value)) return t("不可用", "Unavailable");
  const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"]; let amount = Math.max(0, value); let unit = 0;
  while (amount >= 1024 && unit < units.length - 1) { amount /= 1024; unit += 1; }
  return `${amount >= 100 || unit === 0 ? amount.toFixed(0) : amount.toFixed(1)} ${units[unit]}`;
}
function formatDuration(seconds: bigint): string {
  const days = seconds / 86400n; const hours = seconds % 86400n / 3600n; const minutes = seconds % 3600n / 60n;
  return days > 0n ? t("{0} 天 {1} 小时", "{0}d {1}h", [String(days), String(hours)]) : t("{0} 小时 {1} 分钟", "{0}h {1}m", [String(hours), String(minutes)]);
}

function formatPercent(value: number | null): string { return value === null ? t("不可用", "Unavailable") : `${value.toFixed(1)}%`; }
function formatTime(value: string | null): string {
  if (value === null) return t("尚未上报", "Not yet reported");
  const date = new Date(value);
  return Number.isFinite(date.getTime()) ? date.toLocaleString(getLocale()) : t("未知", "Unknown");
}
