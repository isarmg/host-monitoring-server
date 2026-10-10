import { t } from "@xcss/web/admin-ui/i18n";
import { Button, EmptyState, ErrorState, FormField, LoadingState, TextField } from "@xcss/web/admin-ui";
import { errorRequestId, useAdminApplication } from "@xcss/web/admin-shell";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { isFocusedInstance, isNoContent, LIST_REQUEST_BUDGET, type ClientInstanceListResponse } from "./api";
import { HostDetails } from "./HostDetails";

const instancesPath = "/api/v1/monitoring/client-instances";
const trimInstanceName = (value: string) => value.replace(/^\p{White_Space}+|\p{White_Space}+$/gu, "");
const validInstanceName = (value: string) => value.length > 0 && [...value].length <= 32
  && !/[\u0000-\u001f\u007f-\u009f\ud800-\udfff]/u.test(value);
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
  const nameDrafts = useRef(new Map<string, string>());
  useEffect(() => {
    const controller = new AbortController();
    setFailure(null);
    void client.request(`${instancesPath}?instance_id=${encodeURIComponent(instanceId)}`,
      (value): value is ClientInstanceListResponse => isFocusedInstance(value, instanceId),
      { ...LIST_REQUEST_BUDGET, signal: controller.signal })
      .then(value => { if (!controller.signal.aborted) setResponse(value); })
      .catch(error => { if (!controller.signal.aborted) setFailure({ requestId: errorRequestId(error) }); });
    return () => controller.abort();
  }, [client, instanceId, refreshSignal]);

  if (response === null && failure) return <ErrorState requestId={failure.requestId}>{t("无法读取实例详情", "Unable to load instance details")}</ErrorState>;
  if (response === null) return <LoadingState>{t("正在读取实例详情…", "Loading instance details…")}</LoadingState>;
  // An identity recovery rebinds instance_id; the invitation request_id stays stable.
  // Also accept existing links that use the current instance_id.
  const instance = response.instances.find(item => item.request_id === instanceId || item.instance_id === instanceId);
  if (!instance) return <EmptyState>{t("所选实例已不存在，请返回实例列表。", "The selected instance no longer exists. Return to the instance list.")}</EmptyState>;
  const host = response.hosts.find(item => item.id === instance.instance_id);
  // The request ID owns the settings draft; the host ID owns monitoring state.
  const overview = <>
    <section aria-label={t("配对账户信息", "Pairing account information")}><dl className="host-detail-list">
      <dt>{t("账户名", "Account name")}</dt><dd>{instance.display_name}</dd>
      <dt>{t("账户", "Account")}</dt><dd><code>{instance.instance_id}</code></dd>
      <dt>{t("密码", "Password")}</dt><dd><code>{instance.authorization_code}</code></dd>
      <dt>{t("配对状态", "Pairing status")}</dt><dd>{pairingLabels[instance.status]}</dd>
    </dl></section>
    <InstanceNameSettings requestId={instance.request_id} name={instance.display_name} draftCache={nameDrafts.current} changed={changed} />
  </>;
  return <div className="xcss-content-stack">
    {failure && <ErrorState requestId={failure.requestId}>{t("实例详情刷新失败，页面保留上次成功数据。", "Instance details refresh failed. The last successful data is retained.")}</ErrorState>}
    {instance.status === "active" ? <>
      {host?.data_error && <ErrorState>{t("该实例的监控数据无法读取。", "Monitoring data for this instance is unavailable.")}</ErrorState>}
      <HostDetails key={instance.instance_id} hostId={instance.instance_id} hostName={instance.display_name} refreshSignal={refreshSignal} removed={removed} overview={overview} />
    </> : <section className="xcss-content-panel host-overview" aria-label={t("实例概览", "Instance overview")}>{overview}<section className="host-overview-section"><h3>{t("监控状态", "Monitoring status")}</h3><p>{instance.status === "pending" ? t("实例尚未配对，完成客户端配对后将显示监控详情。", "This instance is not paired yet. Monitoring details will appear after client pairing.") : t("实例配对已取消。", "Instance pairing has been cancelled.")}</p></section></section>}
  </div>;
}

function InstanceNameSettings({ requestId, name, draftCache, changed }: { requestId: string; name: string; draftCache: Map<string, string>; changed(): void }) {
  const { client, notify } = useAdminApplication();
  const [draft, setDraft] = useState(() => draftCache.get(requestId) ?? name);
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<{ requestId?: string } | null>(null);
  const normalizedName = trimInstanceName(draft);
  const nameValid = validInstanceName(normalizedName);
  useEffect(() => { setDraft(draftCache.get(requestId) ?? name); setFailure(null); }, [requestId, name, draftCache]);
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (pending || !nameValid || normalizedName === name) return;
    setPending(true); setFailure(null);
    try {
      await client.request(`${instancesPath}/${requestId}`, isNoContent, { method: "PATCH", body: JSON.stringify({ display_name: normalizedName }) });
      draftCache.delete(requestId);
      notify(t("实例名称已保存", "Instance name saved"));
      changed();
    } catch (error) { setFailure({ requestId: errorRequestId(error) }); }
    finally { setPending(false); }
  }
  return <section className="host-overview-section host-instance-settings" aria-label={t("实例设置", "Instance settings")}>
    <form onSubmit={event => void save(event)} aria-busy={pending}>
      <FormField label={t("实例名称", "Instance name")}><TextField name="display_name" value={draft} onChange={event => { setDraft(event.target.value); draftCache.set(requestId, event.target.value); }} required readOnly={pending} /></FormField>
      {!nameValid && <p role="alert">{t("名称须为 1–32 个字符，不能包含控制字符。", "Use 1–32 characters without control characters.")}</p>}
      {failure && <ErrorState requestId={failure.requestId}>{t("实例名称未能保存，请重试。", "The instance name could not be saved. Please retry.")}</ErrorState>}
      <div className="xcss-actions"><Button type="submit" disabled={pending || !nameValid || normalizedName === name}>{pending ? t("正在保存…", "Saving…") : t("保存名称", "Save name")}</Button></div>
    </form>
  </section>;
}
