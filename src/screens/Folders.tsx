import { useCallback, useEffect, useState } from "react";
import { addFolder, listFolders, pickFolder, removeFolder, type Folder } from "../api";
import ConfirmDialog from "../components/ConfirmDialog";
import { Empty, Screen } from "./Placeholder";

const errMsg = (e: unknown) => (typeof e === "string" ? e : (e as Error)?.message ?? "Unknown error");

export default function Folders() {
  const [folders, setFolders] = useState<Folder[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<Folder | null>(null);

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
  }, [refresh]);

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
    <Screen title="Folders" subtitle="Add or remove folders and see their scan status.">
      <div className="toolbar">
        <button className="btn primary" onClick={onAdd} disabled={busy}>
          + Add folder
        </button>
        <span className="muted">Removing a folder never deletes any files.</span>
      </div>

      {error && (
        <div className="alert" role="alert">
          <span>{error}</span>
          <button className="btn ghost" onClick={() => setError(null)} aria-label="Dismiss">
            ✕
          </button>
        </div>
      )}

      {folders === null ? null : folders.length === 0 ? (
        <Empty text="No folders added yet. Add one to start finding duplicates." />
      ) : (
        <ul className="folder-list">
          {folders.map((f) => (
            <li className="card folder" key={f.id}>
              <div className="folder-info">
                <span className="folder-path" title={f.path}>{f.path}</span>
                <span className="folder-meta">
                  Added {new Date(f.added_at * 1000).toLocaleDateString()} ·{" "}
                  {f.exists ? (
                    <span className="badge">Not scanned</span>
                  ) : (
                    <span className="badge danger">Missing</span>
                  )}
                </span>
              </div>
              <button className="btn" onClick={() => setPending(f)} aria-label={`Remove ${f.path}`}>
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}

      <ConfirmDialog
        open={pending !== null}
        title="Remove folder?"
        confirmLabel="Remove"
        onConfirm={() => pending && void onRemove(pending)}
        onCancel={() => setPending(null)}
      >
        <p className="folder-path">{pending?.path}</p>
        <p className="muted">
          Sift will stop tracking this folder and forget its scan results. Your files
          are not deleted.
        </p>
      </ConfirmDialog>
    </Screen>
  );
}
