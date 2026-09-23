import { t } from "@sarmg/admin-ui/i18n";
import { errorRequestId, useAdminApplication } from "@sarmg/admin-shell";
import { Button, EmptyState, ErrorState, FormField, LoadingState, Table, TextField } from "@sarmg/admin-ui";
import { useEffect, useState } from "react";
import {
  CURRENT_API_PREFIX, isReportLogCalendar, isReportLogsResponse,
  type Host, type ReportLogsResponse,
} from "./api";

function validDate(value: string): boolean {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return false;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const parsed = new Date(0);
  parsed.setUTCFullYear(year, month - 1, day);
  return parsed.getUTCFullYear() === year
    && parsed.getUTCMonth() === month - 1
    && parsed.getUTCDate() === day;
}

export function HostLogs({ host, refreshSignal }: { host: Host; refreshSignal: number }) {
  const { client } = useAdminApplication();
  const [date, setDate] = useState<string | null>(null);
  const [calendarFailure, setCalendarFailure] = useState<{ requestId?: string } | null>(null);
  const [calendarRefresh, setCalendarRefresh] = useState(0);
  const [logs, setLogs] = useState<ReportLogsResponse | null>(null);
  const [failure, setFailure] = useState<{ requestId?: string } | null>(null);
  const [loading, setLoading] = useState(false);
  const [logRefresh, setLogRefresh] = useState(0);

  useEffect(() => {
    const controller = new AbortController();
    setDate(null);
    setCalendarFailure(null);
    void client.request(`${CURRENT_API_PREFIX}/monitoring/logs/calendar`, isReportLogCalendar, { signal: controller.signal })
      .then(value => { if (!controller.signal.aborted) setDate(value.today); })
      .catch(error => { if (!controller.signal.aborted) setCalendarFailure({ requestId: errorRequestId(error) }); });
    return () => controller.abort();
  }, [client, host.id, calendarRefresh]);

  useEffect(() => {
    if (date === null || !validDate(date)) return;
    const controller = new AbortController();
    const requestOptions: RequestInit & { maxResponseBytes: number; timeoutMs: number } = {
      signal: controller.signal,
      maxResponseBytes: 64 * 1024 * 1024,
      timeoutMs: 120_000,
    };
    setLoading(true);
    setFailure(null);
    void client.request(
      `${CURRENT_API_PREFIX}/monitoring/hosts/${host.id}/reports?date=${encodeURIComponent(date)}`,
      isReportLogsResponse,
      requestOptions,
    )
      .then(value => { if (!controller.signal.aborted) setLogs(value); })
      .catch(error => { if (!controller.signal.aborted) setFailure({ requestId: errorRequestId(error) }); })
      .finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, [client, host.id, date, refreshSignal, logRefresh]);

  const current = logs?.host_id === host.id && logs.date === date ? logs : null;
  return <section className="sarmg-content-stack" aria-label={t("上报日志", "Report logs")}>
    <h2>{t("上报日志", "Report logs")}</h2>
    <p>{t("按服务端接收日期列出全部上报。采集时间由客户端提供，可能早于接收时间。", "All reports are grouped by server receipt date. Collection time comes from the client and may be earlier.")}</p>
    <div className="sarmg-content-panel sarmg-content-stack">
      <p>{t("实例：{0}", "Instance: {0}", [host.name])}</p>
      <FormField label={t("日志日期（服务器时区）", "Log date (server time zone)")}>
        <TextField type="date" value={date ?? ""} disabled={date === null && calendarFailure === null}
          aria-invalid={date !== null && !validDate(date) || undefined}
          onChange={event => { setDate(event.target.value); setCalendarFailure(null); }} />
      </FormField>
      <div className="sarmg-actions"><Button disabled={loading || date === null || !validDate(date)}
        onClick={() => setLogRefresh(value => value + 1)}>{t("刷新日志", "Refresh logs")}</Button></div>
    </div>
    {date === null && !calendarFailure && <LoadingState>{t("正在读取服务器当天日期…", "Loading the server's current date…")}</LoadingState>}
    {calendarFailure && <ErrorState requestId={calendarFailure.requestId} onRetry={() => setCalendarRefresh(value => value + 1)}>{t("无法读取服务器当天日期。", "Unable to load the server's current date.")}</ErrorState>}
    {date !== null && !validDate(date) && <p role="alert">{t("请选择有效的日志日期。", "Choose a valid log date.")}</p>}
    {failure && <ErrorState requestId={failure.requestId} onRetry={() => setLogRefresh(value => value + 1)}>{t("无法读取上报日志。", "Unable to load report logs.")}</ErrorState>}
    {date !== null && validDate(date) && loading && <LoadingState>{t("正在读取所选日期的全部日志…", "Loading all logs for the selected date…")}</LoadingState>}
    {date !== null && validDate(date) && !loading && !failure && current && (current.reports.length === 0
      ? <EmptyState>{t("所选日期没有上报", "No reports on the selected date")}</EmptyState>
      : <Table><thead><tr><th>{t("服务端接收时间", "Server receipt time")}</th><th>{t("客户端采集时间", "Client collection time")}</th><th>{t("报告 ID", "Report ID")}</th></tr></thead><tbody>
        {current.reports.map(report => <tr key={report.report_id}><td>{report.received_at_server}</td><td>{report.collected_at_server}</td><td><code>{report.report_id}</code></td></tr>)}
      </tbody></Table>)}
  </section>;
}
