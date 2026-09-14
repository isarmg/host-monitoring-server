import { displayLabel } from "./display-labels";
import { t, getLocale } from "@sarmg/admin-ui/i18n";
import { InstanceNameField } from "@sarmg/admin-shell";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { Button, Dialog, ErrorState, FormField, TextField, Table, EmptyState, LoadingState, ConfirmDangerDialog } from "@sarmg/admin-ui";
import { errorRequestId, useAdminApplication } from "@sarmg/admin-shell";
import { isActivation, isCreatedInstance, isInstance, isInstances, isNoContent, isPairingSummary, isUuid,
  type ClientInstance, type CreatedInstance, type Host, type PairingSummary } from "./api";

const instancesPath = "/api/v2/monitoring/client-instances";
const labels = { pending: t("待配对", "Awaiting pairing"), active: t("已配对", "Paired"), cancelled: t("已取消", "Cancelled") };
type Failure = { requestId?: string };
function randomAuthorizationCode(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return `uci_${Array.from(bytes, byte => byte.toString(16).padStart(2, "0")).join("")}`;
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

export function Instances({ hosts, totalHosts, hostsChanged, select, openCreateSignal = 0, refreshSignal = 0 }: { hosts: Host[]; totalHosts: number; hostsChanged(): void; select(id: string): void; openCreateSignal?: number; refreshSignal?: number }) {
  const { client, notify } = useAdminApplication();
  const [rows, setRows] = useState<ClientInstance[] | null>(null);
  const [failure, setFailure] = useState<Failure | null>(null);
  const [generation, setGeneration] = useState(0);
  const [creating, setCreating] = useState(false);
  useEffect(() => { if (openCreateSignal > 0) setCreating(true); }, [openCreateSignal]);
  const [cancelling, setCancelling] = useState<ClientInstance | null>(null);
  const [rotating, setRotating] = useState<ClientInstance | null>(null);
  const [activation, setActivation] = useState<string | null>(() => {
    const match = /^\/activate\/([^/]+)\/?$/.exec(window.location.pathname);
    return match ? match[1] : null;
  });
  const refresh = () => setGeneration(value => value + 1);
  useEffect(() => {
    const controller = new AbortController(); setRows(null); setFailure(null);
    void client.request(instancesPath, isInstances, { signal: controller.signal })
      .then(value => { if (!controller.signal.aborted) setRows(value); })
      .catch(error => { if (!controller.signal.aborted) setFailure({ requestId: errorRequestId(error) }); });
    return () => controller.abort();
  }, [client, generation, refreshSignal]);
  function closeActivation() {
    setActivation(null);
    if (window.location.pathname.startsWith("/activate/")) window.history.replaceState(null, "", "/#instances");
  }
  const hostById = new Map(hosts.map(host => [host.id, host]));
  const online = hosts.filter(host => host.status === "online").length;
  const pending = rows?.filter(row => row.status === "pending").length ?? 0;
  const paired = rows?.filter(row => row.status === "active").length ?? 0;
  return <div className="sarmg-content-stack"><section className="sarmg-content-stack" aria-labelledby="statistics-heading"><h2 id="statistics-heading">{t("统计", "Statistics")}</h2>
    <Table aria-label={t("实例统计", "Instance statistics")}><thead><tr><th>{t("统计项", "Metric")}</th><th>{t("当前值", "Current value")}</th></tr></thead><tbody>
      <tr><th scope="row">{t("实例总数", "Total instances")}</th><td>{rows?.length ?? totalHosts}</td></tr>
      <tr><th scope="row">{t("在线实例", "Online instances")}</th><td>{online}</td></tr>
      <tr><th scope="row">{t("已配对实例", "Paired instances")}</th><td>{paired}</td></tr>
      <tr><th scope="row">{t("待配对实例", "Instances awaiting pairing")}</th><td>{pending}</td></tr>
    </tbody></Table></section>
    <section className="sarmg-content-stack" aria-labelledby="instances-heading"><h2 id="instances-heading">{t("实例列表", "Instance list")}</h2>
    <p>{t("列表统一显示配对、在线和监控状态。每个实例拥有一个长期授权码；更换后客户端必须重新配对。", "The list combines pairing, online, and monitoring state. Each instance has a long-lived authorization code; changing it requires the client to pair again.")}</p>
    {failure ? <ErrorState requestId={failure.requestId} onRetry={refresh}>{t("无法加载实例", "Unable to load instances")}</ErrorState>
      : rows === null ? <LoadingState>{t("正在加载实例…", "Loading instances…")}</LoadingState>
      : rows.length === 0 ? <EmptyState>{t("暂无实例", "No instances yet")}</EmptyState>
      : <Table aria-label={t("实例列表", "Instance list")}><caption>{t("最近 200 条实例", "Most recent 200 instances")}</caption><thead><tr><th scope="col">{t("名称", "Name")}</th><th scope="col">{t("配对状态", "Pairing status")}</th><th scope="col">{t("在线状态", "Online status")}</th><th scope="col">{t("系统 / 架构", "System / architecture")}</th><th scope="col">{t("授权码", "Authorization code")}</th><th scope="col">{t("操作", "Actions")}</th></tr></thead>
        <tbody>{rows.map(row => { const host = hostById.get(row.instance_id); return <tr key={row.request_id}><th scope="row">{host ? <Button aria-label={t("选择实例 {0}", "Select instance {0}", [row.display_name])} onClick={() => select(host.id)}>{row.display_name}</Button> : row.display_name}</th><td>{labels[row.status]}</td>
        <td>{host ? host.status === "online" ? t("在线", "Online") : t("离线", "Offline") : t("尚未上报", "Not yet reported")}</td><td>{host ? `${host.os} / ${host.arch}` : "—"}</td>
        <td><code>{row.authorization_code}</code></td><td>{row.status !== "cancelled" && <Button onClick={() => setRotating(row)}>{t("更换授权码", "Change code")}</Button>}{(row.status === "pending" || row.status === "cancelled") && <Button onClick={() => setCancelling(row)}>{row.status === "pending" ? t("取消配对", "Cancel pairing") : t("删除实例", "Delete instance")}</Button>}</td></tr>; })}</tbody></Table>}
    {creating && <CreateInstance close={() => setCreating(false)} changed={refresh} />}
    {cancelling && <CancelInstance instance={cancelling} close={() => setCancelling(null)} changed={() => { setCancelling(null); refresh(); }} />}
    {rotating && <RotateInstance instance={rotating} close={() => setRotating(null)} changed={() => { setRotating(null); refresh(); hostsChanged(); notify(t("授权码已更换，客户端必须使用新码重新配对。", "Authorization code changed. The client must pair again with the new code.")); }} />}
    {activation !== null && <ActivateInstance initialId={activation} close={closeActivation} changed={() => {
      closeActivation(); refresh(); hostsChanged(); notify(t("配对已激活，等待 客户端 确认并上报监控数据。", "Pairing activated; waiting for client confirmation and monitoring reports."));
    }} />}
  </section></div>;
}

function RotateInstance({ instance, close, changed }: { instance: ClientInstance; close(): void; changed(): void }) {
  const { client } = useAdminApplication(); const action = useAction();
  return <ConfirmDangerDialog title={t("更换授权码", "Change authorization code")} description={t("更换 {0} 的授权码会立即撤销现有客户端凭据。客户端必须取得新码并重新配对。", "Changing {0}'s authorization code immediately revokes its current client credential. The client must receive the new code and pair again.", [instance.display_name])}
    pending={action.pending} onClose={() => { if (action.idle()) close(); }} onConfirm={() => void action.run(async signal => {
      await client.request(`${instancesPath}/${instance.request_id}/authorization`, isInstance, { method: "PUT", signal, body: JSON.stringify({ authorization_code: randomAuthorizationCode() }) });
      if (!signal.aborted) changed();
    })}>{action.failure && <ErrorState requestId={action.failure.requestId}>{t("更换未能确认，请刷新实例核对授权码和配对状态。", "The change could not be confirmed. Refresh and verify the code and pairing state.")}</ErrorState>}</ConfirmDangerDialog>;
}

function CreateInstance({ close, changed }: { close(): void; changed(): void }) {
  const { client } = useAdminApplication();
  const action = useAction();
  const [result, setResult] = useState<CreatedInstance | null>(null);
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); const data = new FormData(event.currentTarget);
    void action.run(async signal => {
      const value = await client.request(instancesPath, isCreatedInstance, { method: "POST", signal,
        body: JSON.stringify({ display_name: String(data.get("display_name")).trim() }) });
      if (!signal.aborted) { setResult(value); changed(); }
    });
  }
  return <Dialog title={t("新建 客户端 实例", "Create client instance")} onClose={() => { if (action.idle()) close(); }}>
    {result ? <div>
      <p role="status">{t("实例已创建：", "Instance created:")}{result.display_name}</p>
      <p>{t("这是实例的长期授权码，之后仍可在实例列表查看和更换。请勿放入网址或日志。", "This is the instance's long-lived authorization code. It remains viewable and replaceable in the instance list. Never put it in URLs or logs.")}</p>
      <FormField label={t("授权码", "Authorization code")}><TextField readOnly value={result.activation_code} autoComplete="off" onFocus={event => event.currentTarget.select()} /></FormField>
      <p>{t("授权码不会因配对成功而失效；只有更换或取消实例才会失效。", "The code remains valid after pairing and changes only when the instance is rotated or cancelled.")}</p>
      <Button onClick={close}>{t("已保存，关闭", "Saved; close")}</Button>
    </div> : <form onSubmit={submit} aria-busy={action.pending}>
      {action.failure && <ErrorState requestId={action.failure.requestId}>{t("创建未能确认。请先刷新实例核对状态；丢失配对码的实例可取消后重新创建，不要重复提交。", "Creation could not be confirmed. Refresh and check first. If the code is lost, cancel the instance and create a new one; do not submit twice.")}</ErrorState>}
      <FormField label={t("实例名称", "Instance name")}><InstanceNameField name="display_name" required title={t("实例名称最多 32 个字符", "Instance names may contain up to 32 characters")} readOnly={action.pending} data-sarmg-initial-focus /></FormField>
      <p>{t("实例名称最多 32 个字符。", "Instance names may contain up to 32 characters.")}</p>
      <div className="sarmg-actions"><Button disabled={action.pending} onClick={close}>{t("取消", "Cancel")}</Button><Button type="submit" disabled={action.pending}>{action.pending ? t("正在创建…", "Creating…") : t("创建实例", "Create instance")}</Button></div>
    </form>}
  </Dialog>;
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
