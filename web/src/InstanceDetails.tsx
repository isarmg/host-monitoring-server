import { t } from "@sarmg/admin-ui/i18n";
import { Button, EmptyState, ErrorState, FormField, LoadingState } from "@sarmg/admin-ui";
import { errorRequestId, InstanceNameField, useAdminApplication } from "@sarmg/admin-shell";
import { useEffect, useState, type FormEvent } from "react";
import { isInstances, isNoContent, type ClientInstanceListResponse } from "./api";
import { HostDetails } from "./HostDetails";

const instancesPath = "/api/v2/monitoring/client-instances";
const pairingLabels = {
  pending: t("待配对", "Awaiting pairing"),
  active: t("已配对", "Paired"),
  cancelled: t("已取消", "Cancelled"),
};

export function InstanceDetails({ instanceId, refreshSignal, changed, removed }: {
  instanceId: string;
  refreshSignal: number;
  changed(): void;
  removed(): void;
}) {
  const { client } = useAdminApplication();
  const [response, setResponse] = useState<ClientInstanceListResponse | null>(null);
  const [failure, setFailure] = useState<{ requestId?: string } | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    setFailure(null);
    void client.request(instancesPath, isInstances, { signal: controller.signal })
      .then(value => { if (!controller.signal.aborted) setResponse(value); })
      .catch(error => { if (!controller.signal.aborted) setFailure({ requestId: errorRequestId(error) }); });
    return () => controller.abort();
  }, [client, instanceId, refreshSignal]);

  if (failure) return <ErrorState requestId={failure.requestId}>{t("无法读取实例详情", "Unable to load instance details")}</ErrorState>;
  if (response === null) return <LoadingState>{t("正在读取实例详情…", "Loading instance details…")}</LoadingState>;
  const instance = response.instances.find(item => item.instance_id === instanceId);
  if (!instance) return <EmptyState>{t("所选实例已不存在，请返回实例列表。", "The selected instance no longer exists. Return to the instance list.")}</EmptyState>;
  const host = response.hosts.find(item => item.id === instanceId);
  return <div className="sarmg-content-stack">
    <section className="sarmg-content-panel" aria-label={t("配对账户信息", "Pairing account information")}><h2>{instance.display_name}</h2><dl className="host-detail-list">
      <dt>{t("账户名", "Account name")}</dt><dd>{instance.display_name}</dd>
      <dt>{t("账户", "Account")}</dt><dd><code>{instance.instance_id}</code></dd>
      <dt>{t("密码", "Password")}</dt><dd><code>{instance.authorization_code}</code></dd>
      <dt>{t("配对状态", "Pairing status")}</dt><dd>{pairingLabels[instance.status]}</dd>
    </dl></section>
    <InstanceNameSettings requestId={instance.request_id} name={instance.display_name} changed={changed} />
    {host ? <HostDetails hostId={instanceId} refreshSignal={refreshSignal} removed={removed} />
      : <section className="sarmg-content-panel"><h2>{t("监控状态", "Monitoring status")}</h2><p>{instance.status === "pending" ? t("实例尚未配对，完成客户端配对后将显示监控详情。", "This instance is not paired yet. Monitoring details will appear after client pairing.") : t("正在等待客户端首次上报监控数据。", "Waiting for the client’s first monitoring report.")}</p></section>}
  </div>;
}

function InstanceNameSettings({ requestId, name, changed }: { requestId: string; name: string; changed(): void }) {
  const { client, notify } = useAdminApplication();
  const [draft, setDraft] = useState(name);
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<{ requestId?: string } | null>(null);
  useEffect(() => { setDraft(name); setFailure(null); }, [requestId, name]);
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (pending || draft.trim() === name) return;
    setPending(true); setFailure(null);
    try {
      await client.request(`${instancesPath}/${requestId}`, isNoContent, { method: "PATCH", body: JSON.stringify({ display_name: draft.trim() }) });
      notify(t("实例名称已保存", "Instance name saved"));
      changed();
    } catch (error) { setFailure({ requestId: errorRequestId(error) }); }
    finally { setPending(false); }
  }
  return <section className="sarmg-content-panel" aria-label={t("实例设置", "Instance settings")}><h2>{t("实例设置", "Instance settings")}</h2>
    <form onSubmit={event => void save(event)} aria-busy={pending}>
      <FormField label={t("实例名称", "Instance name")}><InstanceNameField name="display_name" value={draft} onChange={event => setDraft(event.target.value)} required readOnly={pending} /></FormField>
      {failure && <ErrorState requestId={failure.requestId}>{t("实例名称未能保存，请重试。", "The instance name could not be saved. Please retry.")}</ErrorState>}
      <div className="sarmg-actions"><Button type="submit" disabled={pending || draft.trim() === name}>{pending ? t("正在保存…", "Saving…") : t("保存名称", "Save name")}</Button></div>
    </form>
  </section>;
}
