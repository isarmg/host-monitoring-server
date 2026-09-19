import { t } from "@sarmg/admin-ui/i18n";
import { createSarmgAdminApplication, errorRequestId, useAdminApplication } from "@sarmg/admin-shell";
import { EmptyState, ErrorState, LoadingState } from "@sarmg/admin-ui";
import { useEffect, useState } from "react";
import { CURRENT_API_PREFIX, administratorApi, isHostListResponse, isUuid, type HostListResponse } from "./api";
import { Instances } from "./Instances";
import { HostDetails } from "./HostDetails";
import { InstanceHeaderActions, InstancePageNavigation, type InstancePage } from "@sarmg/admin-shell";

function currentRoute(): { page: InstancePage; hostId: string | null } {
  const [page, hostId] = window.location.hash.slice(1).split("/");
  return { page: ["details", "logs"].includes(page) ? page as InstancePage : "instances", hostId: isUuid(hostId) ? hostId : null };
}

function HostsPage() {
  const { client } = useAdminApplication();
  const [response, setResponse] = useState<HostListResponse | null>(null);
  const [failure, setFailure] = useState<{ requestId?: string } | null>(null);
  const [generation, setGeneration] = useState(0);
  const [selected, setSelected] = useState<string | null>(() => currentRoute().hostId);
  const [creating, setCreating] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [page, setPage] = useState<InstancePage>(() => currentRoute().page);
  useEffect(() => {
    const changed = () => { const route = currentRoute(); setPage(route.page); if (route.hostId) setSelected(route.hostId); };
    window.addEventListener("hashchange", changed); return () => window.removeEventListener("hashchange", changed);
  }, []);
  useEffect(() => {
    const controller = new AbortController();
    setRefreshing(true); setFailure(null);
    void client.request(`${CURRENT_API_PREFIX}/monitoring/hosts?limit=1000&offset=0`, isHostListResponse, { signal: controller.signal })
      .then(value => { if (!controller.signal.aborted) setResponse(value); })
      .catch(error => { if (!controller.signal.aborted) setFailure({ requestId: errorRequestId(error) }); })
      .finally(() => { if (!controller.signal.aborted) setRefreshing(false); });
    return () => controller.abort();
  }, [client, generation]);
  const refresh = () => setGeneration(value => value + 1);
  const host = selected ? response?.hosts.find(item => item.id === selected) : response?.hosts[0];
  const hostId = selected ?? response?.hosts[0]?.id ?? null;
  return <section id="hosts" className="sarmg-content-stack"><InstanceHeaderActions create={() => { window.location.hash = "instances"; setCreating(true); }} refresh={refresh} refreshing={refreshing} /><InstancePageNavigation page={page} detailsDisabled={!hostId} navigate={value => { window.location.hash = hostId && value !== "instances" ? `${value}/${hostId}` : value; }} /><h1 className="sarmg-visually-hidden">{t("主机监控", "Host monitoring")}</h1>
      {failure && <ErrorState requestId={failure.requestId} onRetry={refresh}>{t("无法加载主机列表", "Unable to load hosts")}</ErrorState>}
      <div className="sarmg-content-stack" hidden={page !== "instances"}>
      {response === null ? failure ? <EmptyState>{t("请重试加载实例列表", "Retry loading the instance list")}</EmptyState> : <LoadingState>{t("正在加载主机…", "Loading hosts…")}</LoadingState>
        : <Instances creating={creating} closeCreate={() => setCreating(false)} refreshSignal={generation} hostsChanged={refresh} select={id => { setSelected(id); window.location.hash = `details/${id}`; }} />}
      </div>
      {page === "details" && <section className="sarmg-content-stack" aria-label={t("详细信息与设置", "Details and settings")}>
      {response === null ? failure ? <EmptyState>{t("请重试加载实例列表", "Retry loading the instance list")}</EmptyState> : <LoadingState>{t("正在加载主机…", "Loading hosts…")}</LoadingState>
        : hostId ? <HostDetails key={hostId} hostId={hostId} changed={refresh} /> : <EmptyState>{t("暂无主机，请点击“新建实例”并完成配对。", "No hosts yet. Create an instance and complete pairing.")}</EmptyState>}
      </section>}
      {page === "logs" && <section className="sarmg-content-stack"><h2>{t("上报时间", "Report timestamps")}</h2>{host ? <dl className="sarmg-content-panel"><dt>{t("实例", "Instance")}</dt><dd>{host.name}</dd><dt>{t("最近客户端采集", "Latest client collection")}</dt><dd>{host.latest_collected_at ?? t("暂无上报", "No reports")}</dd><dt>{t("服务端最后接收", "Last server receipt")}</dt><dd>{host.last_seen_at}</dd></dl> : <EmptyState>{t("所选实例不在当前摘要页，请返回详情读取。", "The selected instance is not in the current summary page; open its details instead.")}</EmptyState>}</section>}
  </section>;
}

export default createSarmgAdminApplication({
  product: { name: "Host Monitoring" },
  client: administratorApi,
  navigation: [],
  routes: <HostsPage />,
});
