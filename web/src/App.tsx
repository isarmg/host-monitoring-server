import { t } from "@xcss/web/admin-ui/i18n";
import { createXcssAdminApplication, errorRequestId, useAdminApplication } from "@xcss/web/admin-shell";
import { EmptyState, ErrorState, LoadingState } from "@xcss/web/admin-ui";
import { useEffect, useState } from "react";
import { CURRENT_API_PREFIX, LIST_REQUEST_BUDGET, administratorApi, isCreatedInstance, isFocusedInstance, isHostListResponse, isUuid, type ClientInstanceListResponse, type Host, type HostListResponse } from "./api";
import { Instances } from "./Instances";
import { InstanceDetails } from "./InstanceDetails";
import { HostLogs } from "./HostLogs";
import { InstancePageNavigation, InstanceHeaderActions, type InstancePage, AccountPage } from "@xcss/web/admin-shell";

type HostPage = InstancePage | "account";

function currentRoute(): { page: HostPage; hostId: string | null } {
  const [page, hostId] = window.location.hash.slice(1).split("/");
  return { page: ["details", "logs", "account"].includes(page) ? page as HostPage : "instances", hostId: isUuid(hostId) ? hostId : null };
}

function HostsPage() {
  const { client, notify } = useAdminApplication();
  const [response, setResponse] = useState<HostListResponse | null>(null);
  const [logHost, setLogHost] = useState<{ selection: string; host: Host | null } | null>(null);
  const [logFailure, setLogFailure] = useState<{ requestId?: string } | null>(null);
  const [failure, setFailure] = useState<{ requestId?: string } | null>(null);
  const [generation, setGeneration] = useState(0);
  const [selected, setSelected] = useState<string | null>(() => currentRoute().hostId);
  const [creating, setCreating] = useState(false);
  const [createFailure, setCreateFailure] = useState<{ requestId?: string } | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [page, setPage] = useState<HostPage>(() => currentRoute().page);
  useEffect(() => {
    const changed = () => { const route = currentRoute(); setPage(route.page); if (route.hostId) setSelected(route.hostId); };
    window.addEventListener("hashchange", changed); return () => window.removeEventListener("hashchange", changed);
  }, []);
  useEffect(() => {
    if (page !== "instances") { setRefreshing(false); return; }
    const controller = new AbortController();
    setRefreshing(true); setFailure(null);
    void client.request(`${CURRENT_API_PREFIX}/monitoring/hosts`, isHostListResponse, { ...LIST_REQUEST_BUDGET, signal: controller.signal })
      .then(hosts => { if (!controller.signal.aborted) setResponse(hosts); })
      .catch(error => { if (!controller.signal.aborted) setFailure({ requestId: errorRequestId(error) }); })
      .finally(() => { if (!controller.signal.aborted) setRefreshing(false); });
    return () => controller.abort();
  }, [client, generation, page]);
  useEffect(() => {
    if (page !== "logs" || selected === null) return;
    const controller = new AbortController();
    setLogHost(current => current?.selection === selected ? current : null); setLogFailure(null);
    void client.request(`${CURRENT_API_PREFIX}/monitoring/client-instances?instance_id=${encodeURIComponent(selected)}`,
      (value): value is ClientInstanceListResponse => isFocusedInstance(value, selected),
      { ...LIST_REQUEST_BUDGET, signal: controller.signal })
      .then(value => {
        if (controller.signal.aborted) return;
        const instance = value.instances[0];
        setLogHost({ selection: selected, host: value.hosts.find(host => host.id === instance?.instance_id) ?? null });
      })
      .catch(error => { if (!controller.signal.aborted) setLogFailure({ requestId: errorRequestId(error) }); });
    return () => controller.abort();
  }, [client, generation, page, selected]);
  const refresh = () => { setResponse(null); setGeneration(value => value + 1); };
  async function createInstance() {
    if (creating) return;
    setCreating(true); setCreateFailure(null); window.location.hash = "instances";
    try {
      await client.request("/api/v1/monitoring/client-instances", isCreatedInstance, { method: "POST", body: JSON.stringify({ display_name: t("新实例", "New instance") }) });
      notify(t("实例已创建，密码可在详细信息中查看。", "Instance created. Its password is available in details."));
      refresh();
    } catch (error) { setCreateFailure({ requestId: errorRequestId(error) }); }
    finally { setCreating(false); }
  }
  const removed = () => {
    setSelected(null);
    window.location.hash = "instances";
    refresh();
  };
  const hostId = selected ?? response?.hosts[0]?.id ?? null;
  return <section id="hosts" className="xcss-content-stack"><InstanceHeaderActions create={() => void createInstance()} refresh={refresh} refreshing={refreshing || creating} /><InstancePageNavigation page={page} detailsDisabled={!hostId} navigate={value => { window.location.hash = hostId && value !== "instances" ? `${value}/${hostId}` : value; }} /><h1 className="xcss-visually-hidden">{t("主机监控", "Host monitoring")}</h1>
      {page !== "account" && createFailure && <ErrorState requestId={createFailure.requestId}>{t("实例未能创建，请刷新列表核对后重试。", "The instance could not be created. Refresh the list before retrying.")}</ErrorState>}
      {page !== "account" && failure && <ErrorState requestId={failure.requestId} onRetry={refresh}>{t("无法加载主机列表", "Unable to load hosts")}</ErrorState>}
      {page === "account" && <AccountPage key={generation} />}
      {page === "instances" && <div className="xcss-content-stack">
      {response === null ? failure ? <EmptyState>{t("请重试加载实例列表", "Retry loading the instance list")}</EmptyState> : <LoadingState>{t("正在加载主机…", "Loading hosts…")}</LoadingState>
        : <Instances refreshSignal={generation} statistics={response.statistics} hostsChanged={refresh} select={id => { setSelected(id); window.location.hash = `details/${id}`; }} />}
      </div>}
      {page === "details" && <section className="xcss-content-stack" aria-label={t("详细信息与设置", "Details and settings")}>
      {hostId ? <InstanceDetails key={hostId} instanceId={hostId} refreshSignal={generation} changed={refresh} removed={removed} /> : <EmptyState>{t("暂无实例，请点击“新建实例”。", "No instances yet. Create an instance.")}</EmptyState>}
      </section>}
      {page === "logs" && (logFailure ? <ErrorState requestId={logFailure.requestId} onRetry={refresh}>{t("无法读取所选实例。", "Unable to load the selected instance.")}</ErrorState>
        : logHost?.selection !== selected ? <LoadingState>{t("正在读取所选实例…", "Loading the selected instance…")}</LoadingState>
        : logHost.host?.data_error ? <ErrorState>{t("该实例的监控数据无法读取。", "Monitoring data for this instance is unavailable.")}</ErrorState>
        : logHost.host ? <HostLogs key={logHost.host.id} host={logHost.host} refreshSignal={generation} /> : <EmptyState>{t("所选实例尚无上报记录。", "The selected instance has no report records yet.")}</EmptyState>)}
  </section>;
}

export default createXcssAdminApplication({
  product: { name: "xsos" },
  client: administratorApi,
  navigation: [],
  loginLandingHref: "#instances",
  routes: <HostsPage />,
});
