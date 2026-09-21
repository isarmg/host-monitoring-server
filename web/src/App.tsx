import { t } from "@sarmg/admin-ui/i18n";
import { createSarmgAdminApplication, errorRequestId, useAdminApplication } from "@sarmg/admin-shell";
import { EmptyState, ErrorState, LoadingState } from "@sarmg/admin-ui";
import { useEffect, useState } from "react";
import { CURRENT_API_PREFIX, administratorApi, isCreatedInstance, isHostListResponse, isUuid, type HostListResponse } from "./api";
import { Instances } from "./Instances";
import { InstanceDetails } from "./InstanceDetails";
import { InstanceHeaderActions, InstancePageNavigation, type InstancePage } from "@sarmg/admin-shell";

function currentRoute(): { page: InstancePage; hostId: string | null } {
  const [page, hostId] = window.location.hash.slice(1).split("/");
  return { page: ["details", "logs"].includes(page) ? page as InstancePage : "instances", hostId: isUuid(hostId) ? hostId : null };
}

function HostsPage() {
  const { client, notify } = useAdminApplication();
  const [response, setResponse] = useState<HostListResponse | null>(null);
  const [failure, setFailure] = useState<{ requestId?: string } | null>(null);
  const [generation, setGeneration] = useState(0);
  const [selected, setSelected] = useState<string | null>(() => currentRoute().hostId);
  const [creating, setCreating] = useState(false);
  const [createFailure, setCreateFailure] = useState<{ requestId?: string } | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [page, setPage] = useState<InstancePage>(() => currentRoute().page);
  useEffect(() => {
    const changed = () => { const route = currentRoute(); setPage(route.page); if (route.hostId) setSelected(route.hostId); };
    window.addEventListener("hashchange", changed); return () => window.removeEventListener("hashchange", changed);
  }, []);
  useEffect(() => {
    const controller = new AbortController();
    setRefreshing(true); setFailure(null);
    void client.request(`${CURRENT_API_PREFIX}/monitoring/hosts`, isHostListResponse, { signal: controller.signal })
      .then(value => { if (!controller.signal.aborted) setResponse(value); })
      .catch(error => { if (!controller.signal.aborted) setFailure({ requestId: errorRequestId(error) }); })
      .finally(() => { if (!controller.signal.aborted) setRefreshing(false); });
    return () => controller.abort();
  }, [client, generation]);
  const refresh = () => setGeneration(value => value + 1);
  async function createInstance() {
    if (creating) return;
    setCreating(true); setCreateFailure(null); window.location.hash = "instances";
    try {
      await client.request("/api/v2/monitoring/client-instances", isCreatedInstance, { method: "POST", body: JSON.stringify({ display_name: t("新实例", "New instance") }) });
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
  const host = selected ? response?.hosts.find(item => item.id === selected) : response?.hosts[0];
  const hostId = selected ?? response?.hosts[0]?.id ?? null;
  return <section id="hosts" className="sarmg-content-stack"><InstanceHeaderActions create={() => void createInstance()} refresh={refresh} refreshing={refreshing || creating} /><InstancePageNavigation page={page} detailsDisabled={!hostId} navigate={value => { window.location.hash = hostId && value !== "instances" ? `${value}/${hostId}` : value; }} /><h1 className="sarmg-visually-hidden">{t("主机监控", "Host monitoring")}</h1>
      {createFailure && <ErrorState requestId={createFailure.requestId}>{t("实例未能创建，请刷新列表核对后重试。", "The instance could not be created. Refresh the list before retrying.")}</ErrorState>}
      {failure && <ErrorState requestId={failure.requestId} onRetry={refresh}>{t("无法加载主机列表", "Unable to load hosts")}</ErrorState>}
      <div className="sarmg-content-stack" hidden={page !== "instances"}>
      {response === null ? failure ? <EmptyState>{t("请重试加载实例列表", "Retry loading the instance list")}</EmptyState> : <LoadingState>{t("正在加载主机…", "Loading hosts…")}</LoadingState>
        : <Instances refreshSignal={generation} statistics={response.statistics} hostsChanged={refresh} select={id => { setSelected(id); window.location.hash = `details/${id}`; }} />}
      </div>
      {page === "details" && <section className="sarmg-content-stack" aria-label={t("详细信息与设置", "Details and settings")}>
      {response === null ? failure ? <EmptyState>{t("请重试加载实例列表", "Retry loading the instance list")}</EmptyState> : <LoadingState>{t("正在加载主机…", "Loading hosts…")}</LoadingState>
        : hostId ? <InstanceDetails key={hostId} instanceId={hostId} refreshSignal={generation} changed={refresh} removed={removed} /> : <EmptyState>{t("暂无实例，请点击“新建实例”。", "No instances yet. Create an instance.")}</EmptyState>}
      </section>}
      {page === "logs" && <section className="sarmg-content-stack"><h2>{t("上报时间", "Report timestamps")}</h2>{host ? <dl className="sarmg-content-panel"><dt>{t("实例", "Instance")}</dt><dd>{host.name}</dd><dt>{t("最近客户端采集", "Latest client collection")}</dt><dd>{host.latest_collected_at ?? t("暂无上报", "No reports")}</dd><dt>{t("服务端最后接收", "Last server receipt")}</dt><dd>{host.last_seen_at}</dd></dl> : <EmptyState>{t("所选实例不在当前摘要页，请返回详情读取。", "The selected instance is not in the current summary page; open its details instead.")}</EmptyState>}</section>}
  </section>;
}

export default createSarmgAdminApplication({
  product: { name: "Host Monitoring" },
  client: administratorApi,
  navigation: [],
  loginLandingHref: "#instances",
  routes: <HostsPage />,
});
