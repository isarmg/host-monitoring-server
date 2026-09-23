import { displayLabel } from "./display-labels";
import { t, getLocale } from "@sarmg/admin-ui/i18n";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { Button, Dialog, ErrorState, FormField, TextField, Table, EmptyState, LoadingState, ConfirmDangerDialog } from "@sarmg/admin-ui";
import { errorRequestId, useAdminApplication } from "@sarmg/admin-shell";
import { isActivation, isInstance, isInstances, isNoContent, isPairingSummary, isUuid,
  type ClientInstance, type ClientInstanceListResponse, type HostStatistics, type PairingSummary } from "./api";

const instancesPath = "/api/v2/monitoring/client-instances";
const labels = { pending: t("待配对", "Awaiting pairing"), active: t("已配对", "Paired"), cancelled: t("已取消", "Cancelled") };
type Failure = { requestId?: string };
function randomAuthorizationCode(): string {
  const alphabet = "abcdefghijklmnopqrstuvwxyz0123456789";
  let value = "";
  while (value.length < 36) {
    for (const byte of crypto.getRandomValues(new Uint8Array(64))) {
      if (byte < 252) value += alphabet[byte % alphabet.length];
      if (value.length === 36) break;
    }
  }
  return value;
}

// Business request lifetime only; authentication and CSRF stay in Foundation.
function useAction() {
  const ref = useRef<AbortController | null>(null);
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<Failure | null>(null);
  useEffect(() => () => ref.current?.abort(), []);
  async function run(work: (signal: AbortSignal) => Promise<void>) {
    if (ref.current) return;
    const controller = new AbortController(); ref.current = controller;
    setPending(true); setFailure(null);
    try { await work(controller.signal); }
    catch (error) { if (!controller.signal.aborted) setFailure({ requestId: errorRequestId(error) }); }
    finally { if (!controller.signal.aborted) { ref.current = null; setPending(false); } }
  }
  return { pending, failure, run, idle: () => ref.current === null };
}

export function Instances({ hostsChanged, select, statistics, refreshSignal = 0 }: { hostsChanged(): void; select(id: string): void; statistics: HostStatistics; refreshSignal?: number }) {
  const { client, notify } = useAdminApplication();
  const [response, setResponse] = useState<ClientInstanceListResponse | null>(null);
  const [failure, setFailure] = useState<Failure | null>(null);
  const [generation, setGeneration] = useState(0);
  const [cancelling, setCancelling] = useState<ClientInstance | null>(null);
  const [rotating, setRotating] = useState<ClientInstance | null>(null);
  const [deleteCandidate, setDeleteCandidate] = useState<string | null>(null);
  const deletion = useAction();
  const [activation, setActivation] = useState<string | null>(() => {
    const match = /^\/activate\/([^/]+)\/?$/.exec(window.location.pathname);
    return match ? match[1] : null;
  });
  const refresh = () => setGeneration(value => value + 1);
  useEffect(() => {
    const controller = new AbortController(); setFailure(null);
    void client.request(instancesPath, isInstances, { signal: controller.signal })
      .then(value => { if (!controller.signal.aborted) setResponse(value); })
      .catch(error => { if (!controller.signal.aborted) setFailure({ requestId: errorRequestId(error) }); });
    return () => controller.abort();
  }, [client, generation, refreshSignal]);
  function closeActivation() {
    setActivation(null);
    if (window.location.pathname.startsWith("/activate/")) window.history.replaceState(null, "", "/#instances");
  }
  const rows = response?.instances ?? null;
  const hostById = new Map((response?.hosts ?? []).map(host => [host.id, host]));
  const statisticRows: Array<[string, { total: number; online: number }]> = [
    [t("总数", "Total"), statistics.total], ["Windows", statistics.windows], ["Linux", statistics.linux], ["macOS", statistics.macos],
  ];
  return <div className="sarmg-content-stack"><section className="sarmg-content-stack" aria-labelledby="statistics-heading"><h2 id="statistics-heading">{t("统计", "Statistics")}</h2>
    <Table aria-label={t("实例统计", "Instance statistics")}><thead><tr><th>{t("统计项", "Metric")}</th><th>{t("总数 / 在线", "Total / online")}</th></tr></thead><tbody>
      {statisticRows.map(([label, count]) => <tr key={label}><th scope="row">{label}</th><td>{count.total} / {count.online}</td></tr>)}
    </tbody></Table></section>
    <section className="sarmg-content-stack" aria-labelledby="instances-heading"><h2 id="instances-heading">{t("实例列表", "Instance list")}</h2>
    {failure ? <ErrorState requestId={failure.requestId} onRetry={refresh}>{t("无法加载实例", "Unable to load instances")}</ErrorState>
      : rows === null ? <LoadingState>{t("正在加载实例…", "Loading instances…")}</LoadingState>
      : rows.length === 0 ? <EmptyState>{t("暂无实例", "No instances yet")}</EmptyState>
      : <>{deletion.failure && <ErrorState requestId={deletion.failure.requestId}>{t("删除未能确认，请刷新实例列表核对。", "Deletion could not be confirmed. Refresh and check the instance list.")}</ErrorState>}<Table aria-label={t("实例列表", "Instance list")}><thead><tr><th scope="col">{t("实例名称", "Instance name")}</th><th scope="col">{t("配对状态", "Pairing status")}</th><th scope="col">{t("在线状态", "Online status")}</th><th scope="col">{t("操作系统/架构", "Operating system / architecture")}</th><th scope="col">{t("操作", "Actions")}</th><th scope="col">{t("删除", "Delete")}</th></tr></thead>
        <tbody>{rows.map(row => { const host = hostById.get(row.instance_id); return <tr key={row.request_id}><th scope="row"><a className="sarmg-instance-link" aria-label={t("选择实例 {0}", "Select instance {0}", [row.display_name])} href={`#details/${row.request_id}`} onClick={() => select(row.request_id)}>{row.display_name}</a></th>
        <td>{labels[row.status]}</td>
        <td>{host ? displayLabel(host.status) : row.status === "active" ? t("等待首次上报", "Waiting for first report") : "—"}</td><td>{host ? `${host.os} / ${host.arch}` : "—"}</td>
        <td><div className="sarmg-actions">{row.status !== "cancelled" && <Button onClick={() => setRotating(row)}>{t("更换密码", "Change password")}</Button>}{row.status === "pending" && <Button onClick={() => setCancelling(row)}>{t("取消配对", "Cancel pairing")}</Button>}</div></td><td><div className="sarmg-actions">{deleteCandidate === row.request_id ? <><Button disabled={deletion.pending} onClick={() => setDeleteCandidate(null)}>{t("取消", "Cancel")}</Button><Button className="sarmg-danger" disabled={deletion.pending} onClick={() => void deletion.run(async signal => { await client.request(`${instancesPath}/${row.request_id}/delete`, isNoContent, { method: "DELETE", signal }); if (!signal.aborted) { setDeleteCandidate(null); refresh(); hostsChanged(); notify(t("实例已删除", "Instance deleted")); } })}>{deletion.pending ? t("正在删除…", "Deleting…") : t("确认删除", "Confirm delete")}</Button></> : <Button disabled={deletion.pending} onClick={() => setDeleteCandidate(row.request_id)}>{t("删除", "Delete")}</Button>}</div></td></tr>; })}</tbody></Table></>}
    {cancelling && <CancelInstance instance={cancelling} close={() => setCancelling(null)} changed={() => { setCancelling(null); refresh(); }} />}
    {rotating && <RotateInstance instance={rotating} close={() => setRotating(null)} changed={() => { setRotating(null); refresh(); hostsChanged(); notify(t("授权码已更换，客户端必须使用新码重新配对。", "Authorization code changed. The client must pair again with the new code.")); }} />}
    {activation !== null && <ActivateInstance initialId={activation} close={closeActivation} changed={() => {
      closeActivation(); refresh(); hostsChanged(); notify(t("配对已激活，等待 客户端 确认并上报监控数据。", "Pairing activated; waiting for client confirmation and monitoring reports."));
    }} />}
  </section></div>;
}

function RotateInstance({ instance, close, changed }: { instance: ClientInstance; close(): void; changed(): void }) {
  const { client } = useAdminApplication(); const action = useAction();
  return <ConfirmDangerDialog title={t("更换密码", "Change password")} description={t("更换 {0} 的密码会立即撤销现有客户端凭据。客户端必须取得新的授权码并重新配对。", "Changing {0}'s password immediately revokes its current client credential. The client must receive the new authorization code and pair again.", [instance.display_name])}
    pending={action.pending} onClose={() => { if (action.idle()) close(); }} onConfirm={() => void action.run(async signal => {
      await client.request(`${instancesPath}/${instance.request_id}/authorization`, isInstance, { method: "PUT", signal, body: JSON.stringify({ authorization_code: randomAuthorizationCode() }) });
      if (!signal.aborted) changed();
    })}>{action.failure && <ErrorState requestId={action.failure.requestId}>{t("更换未能确认，请刷新实例核对授权码和配对状态。", "The change could not be confirmed. Refresh and verify the code and pairing state.")}</ErrorState>}</ConfirmDangerDialog>;
}

function CancelInstance({ instance, close, changed }: { instance: ClientInstance; close(): void; changed(): void }) {
  const { client } = useAdminApplication(); const action = useAction();
  const deleting = instance.status === "cancelled";
  return <ConfirmDangerDialog title={deleting ? t("删除实例", "Delete instance") : t("取消配对", "Cancel pairing")} description={deleting ? t("永久删除 {0} 的已取消实例信息。", "Permanently delete the cancelled instance {0}.", [instance.display_name]) : t("取消 {0} 的实例后，其配对码将不可再用；取消后仍可从列表永久删除该信息。", "Cancelling instance {0} invalidates its pairing code; the cancelled entry can then be permanently deleted from the list.", [instance.display_name])}
    pending={action.pending} onClose={() => { if (action.idle()) close(); }} onConfirm={() => void action.run(async signal => {
      await client.request(`${instancesPath}/${instance.request_id}`, isNoContent, { method: "DELETE", signal });
      if (!signal.aborted) changed();
    })}>{action.failure && <ErrorState requestId={action.failure.requestId}>{deleting ? t("删除未能确认，请刷新实例核对状态。", "Deletion could not be confirmed. Refresh and check the instance state.") : t("取消未能确认，请刷新实例核对状态。", "Cancellation could not be confirmed. Refresh and check the instance state.")}</ErrorState>}</ConfirmDangerDialog>;
}

function ActivateInstance({ initialId, close, changed }: { initialId: string; close(): void; changed(): void }) {
  const { client } = useAdminApplication(); const action = useAction();
  const [id, setId] = useState(initialId);
  const [details, setDetails] = useState<PairingSummary | null>(null);
  async function inspect(signal: AbortSignal) {
    const value = await client.request(`/api/v2/host-monitor/pairing-requests/${id}`, isPairingSummary, { signal });
    if (value.request_id !== id) throw new Error("Pairing identity mismatch");
    if (!signal.aborted) setDetails(value);
  }
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); const form = event.currentTarget;
    if (!isUuid(id)) return;
    if (!details) { void action.run(inspect); return; }
    const data = new FormData(form);
    void action.run(async signal => {
      try {
        await client.request("/api/v2/host-monitor/activate-admin", isActivation, { method: "POST", signal,
          body: JSON.stringify({ request_id: details.request_id, activation_code: String(data.get("activation_code") ?? "") }) });
        if (!signal.aborted) changed();
      } finally {
        const code = form.elements.namedItem("activation_code");
        if (code instanceof HTMLInputElement) { code.value = ""; if (!signal.aborted) code.focus(); }
      }
    });
  }
  return <Dialog title={t("激活 客户端 配对", "Activate client pairing")} onClose={() => { if (action.idle()) close(); }}>
    <form onSubmit={submit} aria-busy={action.pending}>
      <p>{t("填写 客户端 提供的配对请求标识（不是实例标识），读取并核对设备信息后，输入该实例的长期授权码。", "Enter the pairing request ID provided by the client (not the instance ID). Read and verify the device, then enter that instance's long-lived authorization code.")}</p>
      {action.failure && <ErrorState requestId={action.failure.requestId}>{t("请求未能确认。请检查设备请求是否超时及配对状态；配对码本身不设有效期，输入框已清空。不要盲目重试激活。", "The request could not be confirmed. Check request timeout and pairing state. The code itself has no expiry; the input has been cleared. Do not blindly retry activation.")}</ErrorState>}
      <FormField label={t("配对请求标识", "Pairing request ID")}><TextField required value={id} maxLength={36} readOnly={action.pending} data-sarmg-initial-focus
        pattern="[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}"
        onChange={event => { setId(event.target.value); setDetails(null); }} /></FormField>
      {details && <section aria-label={t("待核对设备", "Device to verify")}><p>{t("系统：", "System:")}{details.os} / {details.arch}</p>
        <p>{t("配对状态：", "Pairing status:")}{displayLabel(details.status)}{t("；设备请求会话截止：", "; device request session deadline:")}{new Date(details.expires_at).toLocaleString(getLocale())}{t("（不是配对码有效期）", "(not the pairing code expiry)")}</p></section>}
      {details?.status === "waiting" && <FormField label={t("配对码", "Pairing code")}><TextField name="activation_code" type="password" required maxLength={256} autoComplete="off" readOnly={action.pending} /></FormField>}
      <div className="sarmg-actions"><Button disabled={action.pending} onClick={close}>{t("取消", "Cancel")}</Button>
        {details && <Button disabled={action.pending} onClick={() => void action.run(inspect)}>{t("刷新配对状态", "Refresh pairing status")}</Button>}
        <Button type="submit" disabled={action.pending || (details !== null && details.status !== "waiting")}>{action.pending ? t("正在处理…", "Processing…") : details ? t("确认设备并激活", "Confirm device and activate") : t("读取配对请求", "Read pairing request")}</Button>
      </div>
    </form>
  </Dialog>;
}
