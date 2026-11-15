import { useCallback, useEffect, useState } from "react";
import { addFolder, listFolders, pickFolder, removeFolder, type Folder } from "../api";
import ConfirmDialog from "../components/ConfirmDialog";
import ScanBar from "../components/ScanBar";
import { formatDateTime } from "../format";
import { useI18n } from "../i18n";
import { useScan } from "../scan/ScanProvider";
import { Empty, Screen } from "./Placeholder";

const errMsg = (e: unknown) => (typeof e === "string" ? e : (e as Error)?.message ?? "Unknown error");

export default function Folders() {
  const [folders, setFolders] = useState<Folder[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<Folder | null>(null);
  const { running, start, version } = useScan();
  const { t } = useI18n();

  const refresh = useCallback(async () => {
    try {
      setFolders(await listFolders());
    } catch (e) {
      setError(errMsg(e));
      setFolders([]);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh, version]);

  async function onAdd() {
    setError(null);
    setBusy(true);
    try {
      const path = await pickFolder();
      if (path) {
        await addFolder(path);
        await refresh();
      }
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setBusy(false);
    }
  }

  async function onRemove(f: Folder) {
    setPending(null);
    setError(null);
    try {
      await removeFolder(f.id);
      await refresh();
    } catch (e) {
      setError(errMsg(e));
    }
  }

  return (
    <Screen title={t("folders.title")} subtitle={t("folders.subtitle")}>
      <div className="toolbar">
        <button className="btn primary" onClick={onAdd} disabled={busy}>
          {t("folders.add")}
        </button>
        <button
          className="btn"
          onClick={() => void start()}
          disabled={running || !folders?.some((f) => f.exists)}
        >
          {running ? t("dashboard.scanning") : t("folders.scanAll")}
        </button>
        <span className="muted">{t("folders.note")}</span>
      </div>

      <ScanBar />

      {error && (
        <div className="alert" role="alert">
          <span>{error}</span>
          <button className="btn ghost" onClick={() => setError(null)} aria-label={t("common.dismiss")}>
            ✕
          </button>
        </div>
      )}

      {folders === null ? null : folders.length === 0 ? (
        <Empty text={t("folders.empty")} />
      ) : (
        <ul className="folder-list">
          {folders.map((f) => (
            <li className="card folder" key={f.id}>
              <div className="folder-info">
                <span className="folder-path" title={f.path}>{f.path}</span>
                <span className="folder-meta">
                  {!f.exists ? (
                    <span className="badge danger">{t("folders.missing")}</span>
                  ) : f.last_scanned_at ? (
                    t("folders.scanned", { n: f.file_count, date: formatDateTime(f.last_scanned_at) })
                  ) : (
                    <span className="badge">{t("folders.notScanned")}</span>
                  )}
                </span>
              </div>
              <button className="btn" onClick={() => setPending(f)} aria-label={t("folders.removeAria", { path: f.path })}>
                {t("common.remove")}
              </button>
            </li>
          ))}
        </ul>
      )}

      <ConfirmDialog
        open={pending !== null}
        title={t("folders.removeTitle")}
        confirmLabel={t("common.remove")}
        onConfirm={() => pending && void onRemove(pending)}
        onCancel={() => setPending(null)}
      >
        <p className="folder-path">{pending?.path}</p>
        <p className="muted">{t("folders.removeBody")}</p>
      </ConfirmDialog>
    </Screen>
  );
}
