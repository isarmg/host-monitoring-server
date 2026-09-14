import { displayLabel } from "./display-labels";
import { t } from "@sarmg/admin-ui/i18n";
import { InstanceNameField } from "@sarmg/admin-shell";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { Button, ConfirmDangerDialog, ErrorState, FormField, TextField } from "@sarmg/admin-ui";
import { errorRequestId, useAdminApplication } from "@sarmg/admin-shell";
import { isNoContent, type Host } from "./api";

export function HostDetails({ host, changed }: { host: Host; changed(): void }) {
  const { client, notify } = useAdminApplication();
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<{ requestId?: string } | null>(null);
  const [deleting, setDeleting] = useState(false);
  const active = useRef<AbortController | null>(null);
  useEffect(() => () => active.current?.abort(), []);
  async function mutate(method: "PATCH" | "DELETE", body?: string) {
    if (active.current) return;
    const controller = new AbortController(); active.current = controller; setPending(true); setFailure(null);
    try {
      await client.request(`/api/v2/monitoring/managed-instances/${host.id}`, isNoContent, { method, body, signal: controller.signal });
      if (!controller.signal.aborted) { setDeleting(false); notify(method === "PATCH" ? t("实例名称已保存", "Instance name saved") : t("实例已移除", "Instance removed")); changed(); }
    } catch (error) { if (!controller.signal.aborted) setFailure({ requestId: errorRequestId(error) }); }
    finally { if (!controller.signal.aborted) { active.current = null; setPending(false); } }
  }
  function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); const data = new FormData(event.currentTarget);
    void mutate("PATCH", JSON.stringify({ remark: String(data.get("remark")).trim() }));
  }
  return <div className="sarmg-instance-detail">
    <section className="sarmg-content-panel"><h2>{host.name}</h2><dl>
      <dt>{t("状态", "Status")}</dt><dd>{displayLabel(host.status)}</dd><dt>{t("系统", "System")}</dt><dd>{host.os} / {host.arch}</dd>
      <dt>CPU</dt><dd>{host.cpu_usage_percent === null ? t("不可用", "Unavailable") : `${host.cpu_usage_percent.toFixed(1)}%`}</dd>
      <dt>{t("内存", "Memory")}</dt><dd>{host.memory_usage_percent === null ? t("不可用", "Unavailable") : `${host.memory_usage_percent.toFixed(1)}%`}</dd>
      <dt>{t("最近上报", "Last report")}</dt><dd>{host.latest_collected_at ?? t("尚未上报", "Not yet reported")}</dd>
    </dl><details><summary>{t("完整采集信息", "All collected information")}</summary><dl>{Object.entries(host).filter(([key]) => !key.endsWith("_version")).map(([key, value]) => <div key={key}>
      <dt>{displayLabel(key)}</dt><dd>{key === "capabilities" ? host.capabilities.map(capability => <p key={capability.name}>{displayLabel(capability.name)}: {capability.available ? t("可用", "Available") : t("不可用", "Unavailable")} · {capability.source} {capability.error_kind ? displayLabel(capability.error_kind) : ""}</p>) : value === null ? t("不可用", "Unavailable") : key === "status" ? displayLabel(String(value)) : String(value)}</dd>
    </div>)}</dl></details></section>
    <section className="sarmg-content-panel" aria-label={t("实例设置", "Instance settings")}><h2>{t("实例设置", "Instance settings")}</h2>
      {failure && !deleting && <ErrorState requestId={failure.requestId}>{t("保存未能确认，请刷新核对实例后再决定是否重试。", "The save could not be confirmed. Refresh and check the instance before retrying.")}</ErrorState>}
      <form onSubmit={save} aria-busy={pending}>
        <FormField label={t("实例名称", "Instance name")}><InstanceNameField name="remark" defaultValue={host.name} required title={t("实例名称最多 32 个字符", "Instance names may contain up to 32 characters")} readOnly={pending} /></FormField>
        <p>{t("实例名称最多 32 个字符。", "Instance names may contain up to 32 characters.")}</p>
        <p>{t("采集周期等 客户端 本地设置由客户端管理，此处不修改客户端配置。", "Collection intervals and other local client settings are managed by the client, not by this page.")}</p>
        <div className="sarmg-actions"><Button disabled={pending} onClick={() => setDeleting(true)}>{t("删除实例", "Delete instance")}</Button><Button type="submit" disabled={pending}>{pending ? t("正在处理…", "Processing…") : t("保存设置", "Save settings")}</Button></div>
      </form>
    </section>
    {deleting && <ConfirmDangerDialog title={t("删除监控实例", "Delete monitoring instance")} description={t("移除 {0} 的监控数据和绑定凭据。该 客户端 需要重新配对才能再次接入。", "Remove monitoring data and bound credentials for {0}. The client must pair again to reconnect.", [host.name])}
      pending={pending} onClose={() => { if (!active.current) { setDeleting(false); setFailure(null); } }} onConfirm={() => void mutate("DELETE")}>
      {failure && <ErrorState requestId={failure.requestId}>{t("删除未能确认，请刷新核对实例状态。", "Deletion could not be confirmed. Refresh and check the instance state.")}</ErrorState>}
    </ConfirmDangerDialog>}
  </div>;
}
