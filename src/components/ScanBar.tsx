import { formatNumber } from "../format";
import { useI18n } from "../i18n";
import { useScan } from "../scan/ScanProvider";

/** Progress / result strip shown while a scan runs and after it ends. */
export default function ScanBar() {
  const { running, progress, summary, error, cancel, dismiss } = useScan();
  const { t } = useI18n();

  if (error) {
    return (
      <div className="alert" role="alert">
        <span>{t("scan.failed", { error })}</span>
        <button className="btn ghost" onClick={dismiss} aria-label={t("common.dismiss")}>✕</button>
      </div>
    );
  }

  if (running) {
    const hashing =
      (progress?.phase === "hashing" || progress?.phase === "fingerprinting") && progress.hash_total > 0;
    const pct = hashing ? Math.round((progress!.hash_done / progress!.hash_total) * 100) : null;
    const label = !progress || progress.phase === "discovering"
      ? t("scan.discovering", { n: formatNumber(progress?.files_found ?? 0) })
      : progress.phase === "hashing"
        ? t("scan.hashing", {
            done: formatNumber(progress.hash_done),
            total: formatNumber(progress.hash_total),
          })
        : progress.phase === "fingerprinting"
          ? t("scan.fingerprinting", {
              done: formatNumber(progress.hash_done),
              total: formatNumber(progress.hash_total),
            })
          : t("scan.finishing");
    return (
      <div className="card scanbar" role="status">
        <div className="scanbar-row">
          <span>{label}</span>
          <button className="btn" onClick={() => void cancel()}>{t("common.cancel")}</button>
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
            {summary.status === "cancelled" ? t("scan.cancelled") : t("scan.complete")} ·{" "}
            {t("scan.summary", {
              seen: formatNumber(summary.files_seen),
              hashed: formatNumber(summary.files_hashed),
              cached: formatNumber(summary.files_cached),
            })}
            {summary.errors > 0 && ` · ${t("scan.skipped", { n: formatNumber(summary.errors) })}`}
          </span>
          <button className="btn ghost" onClick={dismiss} aria-label={t("common.dismiss")}>✕</button>
        </div>
      </div>
    );
  }
  return null;
}
