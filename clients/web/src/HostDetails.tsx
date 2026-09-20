import { displayLabel } from "./display-labels";
import { t, getLocale } from "@sarmg/admin-ui/i18n";
import { InstanceNameField } from "@sarmg/admin-shell";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { Button, ConfirmDangerDialog, ErrorState, FormField, LoadingState } from "@sarmg/admin-ui";
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

export function HostDetails({ hostId, refreshSignal, changed, removed }: { hostId: string; refreshSignal: number; changed(): void; removed(): void }) {
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
  const [draft, setDraft] = useState("");
  const [dirty, setDirty] = useState(false);
  const mutation = useRef<AbortController | null>(null);
  const appliedRefreshSignal = useRef(refreshSignal);

  useEffect(() => {
    let timer: number | undefined;
    let controller: AbortController | undefined;
    let stopped = false;
    let inFlight = false;
    let refreshQueued = false;
    const forceInitialRefresh = appliedRefreshSignal.current !== refreshSignal;
    appliedRefreshSignal.current = refreshSignal;
    async function refresh(force = false) {
      if (stopped || (!force && (paused || document.hidden))) return;
      if (inFlight) { refreshQueued = true; return; }
      inFlight = true;
      refreshQueued = false;
      controller = new AbortController();
      try {
        const value = await client.request(`/api/v2/monitoring/hosts/${hostId}`, isHostDetailResponse, { signal: controller.signal });
        if (stopped || controller.signal.aborted || value.host.id !== hostId) return;
        setDetail(value); setUpdatedAt(new Date()); setFailure(null);
        setDraft(current => dirty ? current : value.host.name);
      } catch (error) {
        if (!stopped && !controller.signal.aborted) setFailure({ requestId: errorRequestId(error) });
      } finally {
        inFlight = false;
        if (stopped || paused || document.hidden) return;
        if (refreshQueued) void refresh();
        else timer = window.setTimeout(refresh, 2_000);
      }
    }
    const visible = () => {
      if (document.hidden || paused) return;
      if (timer !== undefined) { clearTimeout(timer); timer = undefined; }
      if (inFlight) refreshQueued = true;
      else void refresh();
    };
    document.addEventListener("visibilitychange", visible);
    void refresh(forceInitialRefresh);
    return () => { stopped = true; controller?.abort(); if (timer !== undefined) clearTimeout(timer); document.removeEventListener("visibilitychange", visible); };
  }, [client, hostId, paused, dirty, refreshSignal]);

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
  async function mutate(method: "PATCH" | "DELETE", body?: string) {
    if (mutation.current) return;
    const controller = new AbortController(); mutation.current = controller; setPending(true); setFailure(null);
    try {
      await client.request(`/api/v2/monitoring/managed-instances/${hostId}`, isNoContent, { method, body, signal: controller.signal });
      if (!controller.signal.aborted) {
        setDeleting(false); setDirty(false);
        notify(method === "PATCH" ? t("实例名称已保存", "Instance name saved") : t("实例已移除", "Instance removed"));
        if (method === "DELETE") removed(); else changed();
      }
    } catch (error) { if (!controller.signal.aborted) setFailure({ requestId: errorRequestId(error) }); }
    finally { if (!controller.signal.aborted) { mutation.current = null; setPending(false); } }
  }
  function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    void mutate("PATCH", JSON.stringify({ remark: draft.trim() }));
  }

  if (detail === null) return failure ? <ErrorState requestId={failure.requestId}>{t("无法读取实例详情", "Unable to load instance details")}</ErrorState> : <LoadingState>{t("正在读取实例详情…", "Loading instance details…")}</LoadingState>;
  const { host, latest } = detail;
  return <div className="sarmg-content-stack">
    <section className="sarmg-content-panel"><h2>{host.name}</h2><dl>
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
    <section className="sarmg-content-panel" aria-label={t("实例设置", "Instance settings")}><h2>{t("实例设置", "Instance settings")}</h2>
      <form onSubmit={save} aria-busy={pending}>
        <FormField label={t("实例名称", "Instance name")}><InstanceNameField name="remark" value={draft} onChange={event => { setDraft(event.target.value); setDirty(true); }} required title={t("实例名称最多 32 个字符", "Instance names may contain up to 32 characters")} readOnly={pending} /></FormField>
        <p>{t("自动更新不会覆盖正在编辑的名称。", "Automatic updates do not overwrite a name being edited.")}</p>
        <div className="sarmg-actions"><Button disabled={pending} onClick={() => setDeleting(true)}>{t("删除实例", "Delete instance")}</Button><Button type="submit" disabled={pending || !dirty}>{pending ? t("正在处理…", "Processing…") : t("保存设置", "Save settings")}</Button></div>
      </form>
    </section>
    {deleting && <ConfirmDangerDialog title={t("删除监控实例", "Delete monitoring instance")} description={t("移除 {0} 的监控数据和绑定凭据。该客户端需要重新配对才能再次接入。", "Remove monitoring data and bound credentials for {0}. The client must pair again to reconnect.", [host.name])}
      pending={pending} onClose={() => { if (!mutation.current) setDeleting(false); }} onConfirm={() => void mutate("DELETE")} />}
  </div>;
}

function HistoryChart({ response, hours, loading, failure, retry, changeHours }: { response: HistorySeriesResponse | null; hours: number; loading: boolean; failure: Failure | null; retry(): void; changeHours(value: number): void }) {
  const series = response?.points ?? [];
  const hasSamples = series.some(point => point.cpu_usage_percent.avg !== null || point.memory_usage_percent.avg !== null);
  const requestedStart = response === null ? Number.NaN : Date.parse(response.requested_from);
  const requestedEnd = response === null ? Number.NaN : Date.parse(response.requested_to);
  const pointTimes = series.map(point => Date.parse(point.start)).filter(Number.isFinite);
  const start = Number.isFinite(requestedStart) ? requestedStart : Math.min(...pointTimes);
  const end = Number.isFinite(requestedEnd) ? requestedEnd : Math.max(...pointTimes);
  const xAt = (value: string) => {
    const timestamp = Date.parse(value);
    if (!Number.isFinite(timestamp) || !Number.isFinite(start) || !Number.isFinite(end) || end <= start) return 0;
    return Math.max(0, Math.min(100, (timestamp - start) * 100 / (end - start)));
  };
  const paths = (key: "cpu_usage_percent" | "memory_usage_percent") => {
    const result: string[] = [];
    let current: string[] = [];
    for (const point of series) {
      const value = point[key].avg;
      if (value === null) {
        if (current.length > 0) result.push(current.join(" "));
        current = [];
      } else current.push(`${xAt(point.start)},${100 - value}`);
    }
    if (current.length > 0) result.push(current.join(" "));
    return result;
  };
  return <section className="sarmg-content-panel" aria-labelledby="history-heading"><h2 id="history-heading">{t("历史趋势", "History trends")}</h2>
    <div className="sarmg-actions">{[[0.25, "15m"], [1, "1h"], [6, "6h"], [24, "24h"], [168, "7d"], [720, "30d"]].map(([value, label]) => <Button key={label} aria-pressed={hours === value} onClick={() => changeHours(Number(value))}>{label}</Button>)}</div>
    <p>{response ? t("每点 {0} 秒，来源：{1}；横轴按采样时间，缺失区间不连线。", "{0} seconds per point, source: {1}; the horizontal axis uses sample time and missing intervals are not connected.", [String(response.step_seconds), response.source]) : loading ? t("正在读取历史趋势…", "Loading history trends…") : null}</p>
    {failure && <ErrorState requestId={failure.requestId} onRetry={retry}>{t("无法读取所选范围的历史趋势。", "Unable to load history trends for the selected range.")}</ErrorState>}
    {!loading && failure === null && !hasSamples ? <p>{t("所选范围没有历史样本", "No historical samples in this range")}</p> : hasSamples ? <svg viewBox="0 0 100 100" role="img" aria-label={t("CPU 与内存使用率历史图", "CPU and memory usage history chart")} preserveAspectRatio="none" style={{ width: "100%", height: "16rem" }}>
      {paths("cpu_usage_percent").map((points, index) => <polyline key={`cpu-${index}`} points={points} fill="none" stroke="currentColor" vectorEffect="non-scaling-stroke" />)}
      {paths("memory_usage_percent").map((points, index) => <polyline key={`memory-${index}`} points={points} fill="none" stroke="#7a5aa6" vectorEffect="non-scaling-stroke" />)}
    </svg> : null}
  </section>;
}

function LatestDevices({ report }: { report: ClientReport | null }) {
  if (report === null) return <section className="sarmg-content-panel"><h2>{t("最新设备信息", "Latest device information")}</h2><p>{t("等待首次上报", "Waiting for the first report")}</p></section>;
  const groups: Array<[string, unknown]> = [
    [t("处理器", "CPU"), report.system.cpu], [t("内存", "Memory"), report.system.memory],
    [t("网络接口", "Network interfaces"), report.system.networks], [t("磁盘", "Disks"), report.system.disks],
    [t("温度传感器", "Temperature sensors"), report.system.temperatures], [t("显卡", "GPUs"), report.system.gpus],
  ];
  return <section className="sarmg-content-panel"><h2>{t("最新设备信息", "Latest device information")}</h2>
    <p>{t("完整逐设备信息仅代表最新一份报告。", "Complete per-device information represents only the latest report.")}</p>
    {groups.map(([title, value]) => <details key={title}><summary>{title}</summary><pre>{JSON.stringify(value, null, 2)}</pre></details>)}
    <details><summary>{t("采集能力与诊断", "Collection capabilities and diagnostics")}</summary>{report.capabilities.map(capability => <p key={capability.name}>{displayLabel(capability.name)}: {capability.available ? t("可用", "Available") : t("不可用", "Unavailable")} · {capability.source}{capability.error_kind ? ` · ${displayLabel(capability.error_kind)}` : ""}{capability.message ? ` · ${capability.message}` : ""}</p>)}</details>
  </section>;
}

function formatPercent(value: number | null): string { return value === null ? t("不可用", "Unavailable") : `${value.toFixed(1)}%`; }
function formatTime(value: string | null): string { return value === null ? t("尚未上报", "Not yet reported") : new Date(value).toLocaleString(getLocale()); }
