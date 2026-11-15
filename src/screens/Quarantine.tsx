import { useCallback, useEffect, useMemo, useState } from "react";
import {
  deleteQuarantined,
  listQuarantine,
  quarantineThumbUrl,
  restoreQuarantined,
  type CleanupResult,
  type QuarantineItem,
} from "../api";
import ConfirmDialog from "../components/ConfirmDialog";
import { formatBytes, formatDate, splitPath } from "../format";
import { useI18n } from "../i18n";
import { useScan } from "../scan/ScanProvider";
import { Empty, Screen } from "./Placeholder";

const errMsg = (e: unknown) => (typeof e === "string" ? e : (e as Error)?.message ?? "Unknown error");

export default function Quarantine() {
  const { version, invalidate, running } = useScan();
  const { t } = useI18n();
  const [items, setItems] = useState<QuarantineItem[] | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [failures, setFailures] = useState<CleanupResult["failed"]>([]);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const list = await listQuarantine();
      setItems(list);
      setSelected((s) => new Set(list.filter((i) => s.has(i.id)).map((i) => i.id)));
    } catch (e) {
      setError(errMsg(e));
      setItems([]);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh, version]);

  const chosen = useMemo(() => (items ?? []).filter((i) => selected.has(i.id)), [items, selected]);
  const chosenBytes = chosen.reduce((n, i) => n + i.size, 0);
  const totalBytes = (items ?? []).reduce((n, i) => n + i.size, 0);

  async function run(
    fn: (ids: number[]) => Promise<CleanupResult>,
    doneKey: "q.restored" | "q.deleted",
  ) {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const r = await fn(chosen.map((i) => i.id));
      setFailures(r.failed);
      setNotice(t(doneKey, { n: r.done, size: formatBytes(r.bytes) }));
      setSelected(new Set());
      invalidate(); // refreshes this list, Duplicates and the Dashboard
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setBusy(false);
    }
  }

  const toggle = (id: number) =>
    setSelected((s) => {
      const n = new Set(s);
      if (!n.delete(id)) n.add(id);
      return n;
    });

  const disabled = busy || running || chosen.length === 0;

  return (
    <Screen
      title={t("q.title")}
      subtitle={t("q.subtitle")}
    >
      {error && <div className="alert" role="alert">{error}</div>}
      {notice && <div className="notice" role="status">{notice}</div>}
      {failures.length > 0 && (
        <details className="warn">
          <summary>{t("common.failed", { n: failures.length })}</summary>
          <ul className="failures">
            {failures.map((f, i) => (
              <li key={i}><code>{f.path}</code>: {f.reason}</li>
            ))}
          </ul>
        </details>
      )}

      {items === null ? null : items.length === 0 ? (
        <Empty text={t("q.empty")} />
      ) : (
        <>
          <div className="toolbar">
            <button className="btn" onClick={() => setSelected(new Set(items.map((i) => i.id)))}>
              {t("q.selectAll")}
            </button>
            <button className="btn" onClick={() => setSelected(new Set())} disabled={selected.size === 0}>
              {t("common.clearSelection")}
            </button>
            <button className="btn primary" disabled={disabled} onClick={() => void run(restoreQuarantined, "q.restored")}>
              {t("q.restore")}
            </button>
            <button className="btn danger" disabled={disabled} onClick={() => setConfirmDelete(true)}>
              {t("q.delete")}
            </button>
            <span className="muted" aria-live="polite">
              {chosen.length > 0
                ? t("common.selectedInfo", { n: chosen.length, size: formatBytes(chosenBytes) })
                : t("q.summary", { files: t("count.files", { n: items.length }), size: formatBytes(totalBytes) })}
            </span>
          </div>

          <ul className="q-list">
            {items.map((i) => {
              const [folder, name] = splitPath(i.original_path);
              return (
                <li key={i.id} className={selected.has(i.id) ? "card q-item selected" : "card q-item"}>
                  <input
                    type="checkbox"
                    checked={selected.has(i.id)}
                    onChange={() => toggle(i.id)}
                    aria-label={t("card.selectAria", { name })}
                  />
                  <div className="q-thumb">
                    {i.exists && (
                      <img
                        src={quarantineThumbUrl(i.id)}
                        alt=""
                        loading="lazy"
                        decoding="async"
                        onError={(e) => (e.currentTarget.style.visibility = "hidden")}
                      />
                    )}
                  </div>
                  <div className="q-info">
                    <span className="copy-name" title={name}>{name}</span>
                    <span className="copy-folder" title={folder}>{t("q.from", { folder })}</span>
                  </div>
                  <div className="q-meta">
                    <span>{formatBytes(i.size)}</span>
                    <span className="muted">
                      {i.width && i.height ? `${i.width} × ${i.height} · ` : ""}
                      {formatDate(i.moved_at)}
                    </span>
                    {!i.exists && <span className="badge danger">{t("q.missing")}</span>}
                  </div>
                </li>
              );
            })}
          </ul>
        </>
      )}

      <ConfirmDialog
        open={confirmDelete}
        title={t("q.confirmTitle")}
        confirmLabel={t("q.confirmLabel", { n: chosen.length })}
        onCancel={() => setConfirmDelete(false)}
        onConfirm={() => {
          setConfirmDelete(false);
          void run(deleteQuarantined, "q.deleted");
        }}
      >
        <p>{t("q.confirmBody", { n: chosen.length, size: formatBytes(chosenBytes) })}</p>
        <p><strong>{t("q.confirmWarn")}</strong></p>
      </ConfirmDialog>
    </Screen>
  );
}
