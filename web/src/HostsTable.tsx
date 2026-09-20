import { t, getLocale } from "@sarmg/admin-ui/i18n";
import { Button, EmptyState, Table } from "@sarmg/admin-ui";
import type { Host } from "./api";

function percent(value: number | null) { return value === null ? t("未上报", "Not reported") : `${value.toFixed(1)}%`; }
function timestamp(value: string) {
  const date = new Date(value);
  return Number.isFinite(date.getTime()) ? date.toLocaleString(getLocale()) : t("未知", "Unknown");
}
export function HostsTable({ hosts, select }: { hosts: Host[]; select(id: string): void }) {
  if (!hosts.length) return <EmptyState>{t("暂无已配对主机，请新建实例并完成配对。", "No paired hosts yet. Create an instance and complete pairing.")}</EmptyState>;
  return <Table aria-label={t("监控实例列表", "Monitoring instance list")}><thead><tr>
    <th scope="col">{t("实例名称", "Instance name")}</th><th scope="col">{t("状态", "Status")}</th><th scope="col">{t("系统 / 架构", "System / architecture")}</th>
    <th scope="col">{t("CPU 使用率", "CPU usage")}</th><th scope="col">{t("内存使用率", "Memory usage")}</th><th scope="col">{t("最近连接", "Last connection")}</th><th scope="col">{t("最近上报", "Last report")}</th>
  </tr></thead><tbody>{hosts.map(host => <tr key={host.id}>
    <th scope="row"><Button aria-label={t("选择实例 {0}", "Select instance {0}", [host.name])} onClick={() => select(host.id)}>{host.name}</Button></th>
    <td>{host.status === "online" ? t("在线", "Online") : host.status === "offline" ? t("离线", "Offline") : t("未知", "Unknown")}</td>
    <td>{host.os} / {host.arch}</td><td>{percent(host.cpu_usage_percent)}</td><td>{percent(host.memory_usage_percent)}</td>
    <td>{timestamp(host.last_seen_at)}</td><td>{host.latest_collected_at ? timestamp(host.latest_collected_at) : t("尚未上报", "Not yet reported")}</td>
  </tr>)}</tbody></Table>;
}
