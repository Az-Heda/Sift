import { useEffect, useState } from "react";
import { getInsights, getStats, type Insight, type Insights, type Stats } from "../api";
import ScanBar from "../components/ScanBar";
import { formatBytes, formatNumber, splitPath } from "../format";
import { useScan } from "../scan/ScanProvider";
import { Empty, Screen } from "./Placeholder";

export default function Dashboard() {
  const { version, running, start } = useScan();
  const [stats, setStats] = useState<Stats | null>(null);
  const [insights, setInsights] = useState<Insights | null>(null);

  useEffect(() => {
    getStats().then(setStats).catch(() => setStats(null));
    getInsights().then(setInsights).catch(() => setInsights(null));
  }, [version]);

  const cards: [string, string][] = [
    ["Files scanned", stats ? formatNumber(stats.files) : "—"],
    ["Storage scanned", stats ? formatBytes(stats.bytes) : "—"],
    ["Duplicate groups", stats ? formatNumber(stats.groups) : "—"],
    ["Wasted storage", stats ? formatBytes(stats.wasted) : "—"],
  ];

  return (
    <Screen title="Dashboard" subtitle="Scan statistics and potential space savings.">
      <div className="toolbar">
        <button className="btn primary" onClick={() => void start()} disabled={running}>
          {running ? "Scanning…" : "Scan now"}
        </button>
        {stats?.last_scan_at && (
          <span className="muted">
            Last scan {new Date(stats.last_scan_at * 1000).toLocaleString()}
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
          <InsightList title="Most duplicated folders" items={insights.folders} path />
          <InsightList title="Most common duplicate types" items={insights.types} />
        </div>
      )}
      {stats && stats.files === 0 && (
        <Empty text="Add a folder and run a scan to see your results here." />
      )}
    </Screen>
  );
}

function InsightList({ title, items, path }: { title: string; items: Insight[]; path?: boolean }) {
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
              {formatNumber(i.count)} files · {formatBytes(i.wasted)}
            </span>
          </li>
        ))}
      </ol>
    </div>
  );
}
