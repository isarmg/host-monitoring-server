import { t } from "@sarmg/admin-ui/i18n";
import { createSarmgAdminApplication, errorRequestId, useAdminApplication } from "@sarmg/admin-shell";
import { EmptyState, ErrorState, LoadingState } from "@sarmg/admin-ui";
import { useEffect, useState } from "react";
import { CURRENT_API_PREFIX, administratorApi, isHostListResponse, type HostListResponse } from "./api";
import { Instances } from "./Instances";
import { HostDetails } from "./HostDetails";
import { InstanceHeaderActions, InstancePageNavigation, type InstancePage } from "@sarmg/admin-shell";

function HostsPage() {
  const { client } = useAdminApplication();
  const [response, setResponse] = useState<HostListResponse | null>(null);
  const [failure, setFailure] = useState<{ requestId?: string } | null>(null);
  const [generation, setGeneration] = useState(0);
  const [selected, setSelected] = useState<string | null>(null);
  const [createSignal, setCreateSignal] = useState(0);
  const [page, setPage] = useState<InstancePage>(() => ["instances", "details", "logs"].includes(window.location.hash.slice(1)) ? window.location.hash.slice(1) as InstancePage : "instances");
  useEffect(() => {
    const changed = () => setPage(["details", "logs"].includes(window.location.hash.slice(1)) ? window.location.hash.slice(1) as InstancePage : "instances");
    window.addEventListener("hashchange", changed); return () => window.removeEventListener("hashchange", changed);
  }, []);
  useEffect(() => {
    const controller = new AbortController();
    setResponse(null); setFailure(null);
    void client.request(`${CURRENT_API_PREFIX}/monitoring/hosts?limit=1000&offset=0`, isHostListResponse, { signal: controller.signal })
      .then(value => { if (!controller.signal.aborted) setResponse(value); })
      .catch(error => { if (!controller.signal.aborted) setFailure({ requestId: errorRequestId(error) }); });
    return () => controller.abort();
  }, [client, generation]);
  const refresh = () => setGeneration(value => value + 1);
  const host = response?.hosts.find(item => item.id === selected) ?? response?.hosts[0];
  return <section id="hosts" className="sarmg-content-stack"><InstanceHeaderActions create={() => { window.location.hash = "instances"; setCreateSignal(value => value + 1); }} refresh={refresh} refreshing={response === null && failure === null} /><InstancePageNavigation page={page} detailsDisabled={!host} navigate={value => { window.location.hash = value; }} /><h1 className="sarmg-visually-hidden">{t("主机监控", "Host monitoring")}</h1>
      {failure && <ErrorState requestId={failure.requestId} onRetry={refresh}>{t("无法加载主机列表", "Unable to load hosts")}</ErrorState>}
      <div className="sarmg-content-stack" hidden={page !== "instances"}>
      {response === null ? failure ? <EmptyState>{t("请重试加载实例列表", "Retry loading the instance list")}</EmptyState> : <LoadingState>{t("正在加载主机…", "Loading hosts…")}</LoadingState>
        : <Instances hosts={response.hosts} totalHosts={response.total} openCreateSignal={createSignal} refreshSignal={generation} hostsChanged={refresh} select={id => { setSelected(id); window.location.hash = "details"; }} />}
      </div>
      {page === "details" && <section className="sarmg-content-stack" aria-label={t("详细信息与设置", "Details and settings")}>
      {response === null ? failure ? <EmptyState>{t("请重试加载实例列表", "Retry loading the instance list")}</EmptyState> : <LoadingState>{t("正在加载主机…", "Loading hosts…")}</LoadingState>
        : host ? <HostDetails key={host.id} host={host} changed={refresh} /> : <EmptyState>{t("暂无主机，请点击“新建实例”并完成配对。", "No hosts yet. Create an instance and complete pairing.")}</EmptyState>}
      </section>}
      {page === "logs" && <section className="sarmg-content-stack"><h2>{t("日志", "Logs")}</h2>{host ? <dl className="sarmg-content-panel"><dt>{t("实例", "Instance")}</dt><dd>{host.name}</dd><dt>{t("最近客户端上报", "Latest client report")}</dt><dd>{host.latest_collected_at ?? t("暂无上报", "No reports")}</dd><dt>{t("服务端最后接收", "Last server receipt")}</dt><dd>{host.last_seen_at}</dd></dl> : <EmptyState>{t("暂无实例日志", "No instance logs")}</EmptyState>}</section>}
  </section>;
}

export default createSarmgAdminApplication({
  product: { name: "Host Monitoring" },
  client: administratorApi,
  navigation: [],
  routes: <HostsPage />,
});
