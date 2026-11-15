import { useEffect, useState } from "react";
import { getInsights, getStats, type Insight, type Insights, type Stats } from "../api";
import ScanBar from "../components/ScanBar";
import { formatBytes, formatDateTime, formatNumber, splitPath } from "../format";
import { useI18n } from "../i18n";
import { useScan } from "../scan/ScanProvider";
import { Empty, Screen } from "./Placeholder";

export default function Dashboard() {
  const { version, running, start } = useScan();
  const { t } = useI18n();
  const [stats, setStats] = useState<Stats | null>(null);
  const [insights, setInsights] = useState<Insights | null>(null);

  useEffect(() => {
    getStats().then(setStats).catch(() => setStats(null));
    getInsights().then(setInsights).catch(() => setInsights(null));
  }, [version]);

  const cards: [string, string][] = [
    [t("stat.files"), stats ? formatNumber(stats.files) : "—"],
    [t("stat.storage"), stats ? formatBytes(stats.bytes) : "—"],
    [t("stat.groups"), stats ? formatNumber(stats.groups) : "—"],
    [t("stat.wasted"), stats ? formatBytes(stats.wasted) : "—"],
    [t("stat.recovered"), stats ? formatBytes(stats.recovered) : "—"],
    [t("stat.quarantined"), stats ? formatBytes(stats.quarantined) : "—"],
  ];

  return (
    <Screen title={t("dashboard.title")} subtitle={t("dashboard.subtitle")}>
      <div className="toolbar">
        <button className="btn primary" onClick={() => void start()} disabled={running}>
          {running ? t("dashboard.scanning") : t("dashboard.scanNow")}
        </button>
        {stats?.last_scan_at && (
          <span className="muted">
            {t("dashboard.lastScan", { date: formatDateTime(stats.last_scan_at) })}
          </span>
        )}
      </div>
      <ScanBar />
      <div className="stat-grid">
        {cards.map(([label, value]) => (
          <div className="card stat" key={label}>
            <span className="stat-label">{label}</span>
            <span className="stat-value">{value}</span>
          </div>
        ))}
      </div>
      {insights && (insights.folders.length > 0 || insights.types.length > 0) && (
        <div className="insight-grid">
          <InsightList title={t("insight.folders")} items={insights.folders} path />
          <InsightList title={t("insight.types")} items={insights.types} />
        </div>
      )}
      {stats && stats.files === 0 && <Empty text={t("dashboard.empty")} />}
    </Screen>
  );
}

function InsightList({ title, items, path }: { title: string; items: Insight[]; path?: boolean }) {
  const { t } = useI18n();
  return (
    <div className="card insight">
      <h2>{title}</h2>
      <ol>
        {items.map((i) => (
          <li key={i.label}>
            <span className="insight-label" title={i.label}>
              {path ? splitPath(i.label)[1] || i.label : i.label}
              {path && <span className="muted"> {splitPath(i.label)[0]}</span>}
            </span>
            <span className="insight-value">
              {t("count.files", { n: i.count })} · {formatBytes(i.wasted)}
            </span>
          </li>
        ))}
      </ol>
    </div>
  );
}
