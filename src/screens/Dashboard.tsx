import { useEffect, useState } from "react";
import { getStats, type Stats } from "../api";
import ScanBar from "../components/ScanBar";
import { formatBytes, formatNumber } from "../format";
import { useScan } from "../scan/ScanProvider";
import { Empty, Screen } from "./Placeholder";

export default function Dashboard() {
  const { version, running, start } = useScan();
  const [stats, setStats] = useState<Stats | null>(null);

  useEffect(() => {
    getStats().then(setStats).catch(() => setStats(null));
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
      {stats && stats.files === 0 && (
        <Empty text="Add a folder and run a scan to see your results here." />
      )}
    </Screen>
  );
}
