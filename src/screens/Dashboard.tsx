import { Empty, Screen } from "./Placeholder";

const STATS = [
  "Files scanned",
  "Storage scanned",
  "Duplicate groups",
  "Wasted storage",
];

export default function Dashboard() {
  return (
    <Screen title="Dashboard" subtitle="Scan statistics and potential space savings.">
      <div className="stat-grid">
        {STATS.map((s) => (
          <div className="card stat" key={s}>
            <span className="stat-label">{s}</span>
            <span className="stat-value">—</span>
          </div>
        ))}
      </div>
      <Empty text="Add a folder and run a scan to see your results here." />
    </Screen>
  );
}
