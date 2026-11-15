import { useScan } from "../scan/ScanProvider";
import { formatNumber } from "../format";

/** Progress / result strip shown while a scan runs and after it ends. */
export default function ScanBar() {
  const { running, progress, summary, error, cancel, dismiss } = useScan();

  if (error) {
    return (
      <div className="alert" role="alert">
        <span>Scan failed: {error}</span>
        <button className="btn ghost" onClick={dismiss} aria-label="Dismiss">✕</button>
      </div>
    );
  }

  if (running) {
    const hashing = progress?.phase === "hashing" && progress.hash_total > 0;
    const pct = hashing ? Math.round((progress!.hash_done / progress!.hash_total) * 100) : null;
    const label = !progress || progress.phase === "discovering"
      ? `Discovering files… ${formatNumber(progress?.files_found ?? 0)} found`
      : progress.phase === "hashing"
        ? `Hashing ${formatNumber(progress.hash_done)} / ${formatNumber(progress.hash_total)} candidate files`
        : "Finishing up…";
    return (
      <div className="card scanbar" role="status">
        <div className="scanbar-row">
          <span>{label}</span>
          <button className="btn" onClick={() => void cancel()}>Cancel</button>
        </div>
        <div
          className={pct === null ? "bar indeterminate" : "bar"}
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={pct ?? undefined}
        >
          <div className="bar-fill" style={pct === null ? undefined : { width: `${pct}%` }} />
        </div>
      </div>
    );
  }

  if (summary) {
    return (
      <div className="card scanbar" role="status">
        <div className="scanbar-row">
          <span>
            {summary.status === "cancelled" ? "Scan cancelled" : "Scan complete"} ·{" "}
            {formatNumber(summary.files_seen)} files seen · {formatNumber(summary.files_hashed)} hashed ·{" "}
            {formatNumber(summary.files_cached)} from cache
            {summary.errors > 0 && ` · ${formatNumber(summary.errors)} skipped (unreadable or changed)`}
          </span>
          <button className="btn ghost" onClick={dismiss} aria-label="Dismiss">✕</button>
        </div>
      </div>
    );
  }
  return null;
}
