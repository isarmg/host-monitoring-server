import { t } from "@xcss/admin-ui/i18n";
import { errorRequestId, useAdminApplication } from "@xcss/admin-shell";
import { Button, EmptyState, ErrorState, LoadingState, Table } from "@xcss/admin-ui";
import { useEffect, useState } from "react";
import {
  CURRENT_API_PREFIX, isReportLogCalendar, isReportLogsResponse,
  LIST_REQUEST_BUDGET, type Host, type ReportLogsResponse,
} from "./api";
import { DateRangeField, type CalendarDateRange } from "@xcss/admin-ui/date-range";
import "@xcss/admin-ui/date-range.css";
import { PageNavigation } from "./PageNavigation";

export function HostLogs({ host, refreshSignal }: { host: Host; refreshSignal: number }) {
  const { client } = useAdminApplication();
  const [selection, setSelection] = useState<{ range: CalendarDateRange | null; cursor: string | null; refreshSignal: number }>({ range: null, cursor: null, refreshSignal });
  const { range } = selection;
  const [draftValid, setDraftValid] = useState(false);
  const cursor = selection.refreshSignal === refreshSignal ? selection.cursor : null;
  const [calendarFailure, setCalendarFailure] = useState<{ requestId?: string } | null>(null);
  const [calendarRefresh, setCalendarRefresh] = useState(0);
  const [logs, setLogs] = useState<ReportLogsResponse | null>(null);
  const [failure, setFailure] = useState<{ requestId?: string } | null>(null);
  const [loading, setLoading] = useState(false);
  const [logRefresh, setLogRefresh] = useState(0);

  useEffect(() => {
    const controller = new AbortController();
    setSelection({ range: null, cursor: null, refreshSignal }); setLogs(null);
    setCalendarFailure(null);
    void client.request(`${CURRENT_API_PREFIX}/monitoring/logs/calendar`, isReportLogCalendar, { signal: controller.signal })
      .then(value => { if (!controller.signal.aborted) setSelection({ range: { start: value.today, end: value.today }, cursor: null, refreshSignal }); })
      .catch(error => { if (!controller.signal.aborted) setCalendarFailure({ requestId: errorRequestId(error) }); });
    return () => controller.abort();
  }, [client, host.id, calendarRefresh]);

  useEffect(() => {
    if (range === null) return;
    const query = range.start === range.end ? new URLSearchParams({ date: range.start }) : new URLSearchParams({ start_date: range.start, end_date: range.end });
    if (cursor !== null) query.set("cursor", cursor);
    const controller = new AbortController();
    const requestOptions: RequestInit & { maxResponseBytes: number; timeoutMs: number } = {
      ...LIST_REQUEST_BUDGET,
      signal: controller.signal,
    };
    setLoading(true); setLogs(null);
    setFailure(null);
    void client.request(
      `${CURRENT_API_PREFIX}/monitoring/hosts/${host.id}/reports?${query}`,
      (value): value is ReportLogsResponse => isReportLogsResponse(value) && value.host_id === host.id && value.date === range.start && (value.end_date ?? value.date) === range.end,
      requestOptions,
    )
      .then(value => { if (!controller.signal.aborted) setLogs(value); })
      .catch(error => { if (!controller.signal.aborted) setFailure({ requestId: errorRequestId(error) }); })
      .finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, [client, host.id, range, cursor, refreshSignal, logRefresh]);

  const first = () => { setSelection({ range, cursor: null, refreshSignal }); setLogRefresh(value => value + 1); };
  const move = (cursor: string) => setSelection({ range, cursor, refreshSignal });

  const current = logs?.host_id === host.id && logs.date === range?.start && (logs.end_date ?? logs.date) === range?.end ? logs : null;
  return <section className="xcss-content-stack" aria-label={t("上报日志", "Report logs")}>
    <div className="xcss-content-panel xcss-content-stack host-logs-panel">
      <p>{t("实例：{0}", "Instance: {0}", [host.name])}</p>
      <div className="xcss-log-date-controls">
        <label htmlFor="host-log-date-start-year">{t("日志日期范围（服务器时区）", "Log date range (server time zone)")}</label>
        <div className="xcss-actions"><Button disabled={loading || range === null || !draftValid}
          onClick={first}>{t("刷新日志", "Refresh logs")}</Button></div>
        <DateRangeField id="host-log-date" value={range} disabled={range === null && calendarFailure === null} onValidityChange={setDraftValid}
          onApply={range => { setSelection({ range, cursor: null, refreshSignal }); setLogRefresh(value => value + 1); setLogs(null); setFailure(null); setCalendarFailure(null); }} />
      </div>
    </div>
    {range === null && !calendarFailure && <LoadingState>{t("正在读取服务器当天日期…", "Loading the server's current date…")}</LoadingState>}
    {calendarFailure && <ErrorState requestId={calendarFailure.requestId} onRetry={() => setCalendarRefresh(value => value + 1)}>{t("无法读取服务器当天日期。", "Unable to load the server's current date.")}</ErrorState>}
    {failure && <ErrorState requestId={failure.requestId} onRetry={() => setLogRefresh(value => value + 1)}>{t("无法读取上报日志。", "Unable to load report logs.")}</ErrorState>}
    {range !== null && loading && <LoadingState>{t("正在读取日志页…", "Loading the log page…")}</LoadingState>}
    {range !== null && !loading && !failure && current && (current.reports.length === 0
      ? <EmptyState>{t("所选日期范围没有上报", "No reports in the selected date range")}</EmptyState>
      : <Table><thead><tr><th>{t("服务端接收时间", "Server receipt time")}</th><th>{t("客户端采集时间", "Client collection time")}</th><th>{t("报告 ID", "Report ID")}</th></tr></thead><tbody>
        {current.reports.map(report => <tr key={report.report_id}><td>{report.received_at_server}</td><td>{report.collected_at_server}</td><td><code>{report.report_id}</code></td></tr>)}
      </tbody></Table>)}
    <PageNavigation page={current} loading={loading} first={first} move={move} label={t("日志分页", "Log pages")} />
  </section>;
}
