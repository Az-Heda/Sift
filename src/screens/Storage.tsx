import { useEffect, useState } from "react";
import {
  getEmptyFolders,
  getStorage,
  revealFile,
  type CategoryUsage,
  type StorageAnalytics,
  type StoredFile,
} from "../api";
import { formatBytes, formatDate, splitPath } from "../format";
import { useI18n, type MsgKey } from "../i18n";
import { useScan } from "../scan/ScanProvider";
import { Empty, Screen } from "./Placeholder";

const CATEGORY_LABELS: Record<CategoryUsage["category"], MsgKey> = {
  image: "cat.image",
  video: "cat.video",
  audio: "cat.audio",
  document: "cat.document",
  archive: "cat.archive",
  other: "cat.other",
};

const errMsg = (e: unknown) => (typeof e === "string" ? e : (e as Error)?.message ?? "Unknown error");

function FileList({ title, hint, files }: { title: string; hint?: string; files: StoredFile[] }) {
  const { t } = useI18n();
  const [error, setError] = useState<string | null>(null);
  return (
    <div className="card insight">
      <h2>{title}</h2>
      {hint && <p className="muted">{hint}</p>}
      {error && <div className="alert" role="alert">{error}</div>}
      {files.length === 0 ? (
        <span className="muted">{t("storage.none")}</span>
      ) : (
        <ol>
          {files.map((f) => {
            const [folder, name] = splitPath(f.path);
            return (
              <li key={f.id}>
                <span className="insight-label" title={f.path}>
                  {name}
                  <span className="muted"> {folder}</span>
                </span>
                <span className="insight-value">
                  {formatBytes(f.size)} · {formatDate(f.modified)}
                </span>
                <button
                  className="btn small"
                  onClick={() => revealFile(f.id).then(() => setError(null), (e) => setError(errMsg(e)))}
                >
                  {t("common.reveal")}
                </button>
              </li>
            );
          })}
        </ol>
      )}
    </div>
  );
}

export default function Storage() {
  const { version } = useScan();
  const { t } = useI18n();
  const [data, setData] = useState<StorageAnalytics | null>(null);
  const [empty, setEmpty] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    setEmpty(null);
    getStorage()
      .then((d) => current && (setData(d), setError(null)))
      .catch((e) => current && setError(errMsg(e)));
    // Walks the disk: kept separate so the rest of the page shows immediately.
    getEmptyFolders()
      .then((l) => current && setEmpty(l))
      .catch(() => current && setEmpty([]));
    return () => {
      current = false;
    };
  }, [version]);

  const total = data?.categories.reduce((n, c) => n + c.bytes, 0) ?? 0;

  return (
    <Screen title={t("storage.title")} subtitle={t("storage.subtitle")}>
      {error && <div className="alert" role="alert">{error}</div>}
      {data && data.categories.length === 0 && <Empty text={t("storage.empty")} />}
      {data && data.categories.length > 0 && (
        <>
          <div className="card insight">
            <h2>{t("storage.byType")}</h2>
            <ol>
              {data.categories.map((c) => (
                <li key={c.category} className="usage-row">
                  <span className="insight-label">{t(CATEGORY_LABELS[c.category])}</span>
                  <div
                    className="bar"
                    role="img"
                    aria-label={`${Math.round((c.bytes / total) * 100)}%`}
                  >
                    <div className="bar-fill" style={{ width: `${(c.bytes / total) * 100}%` }} />
                  </div>
                  <span className="insight-value">
                    {formatBytes(c.bytes)} · {t("count.files", { n: c.files })}
                  </span>
                </li>
              ))}
            </ol>
          </div>

          <div className="insight-grid">
            <FileList title={t("storage.largest")} files={data.largest} />
            <FileList title={t("storage.old")} hint={t("storage.oldHint")} files={data.old} />
            <div className="card insight">
              <h2>{t("storage.folders")}</h2>
              <ol>
                {data.folders.map((f) => (
                  <li key={f.path}>
                    <span className="insight-label" title={f.path}>
                      {splitPath(f.path)[1] || f.path}
                      <span className="muted"> {splitPath(f.path)[0]}</span>
                    </span>
                    <span className="insight-value">
                      {formatBytes(f.bytes)} · {t("count.files", { n: f.files })}
                    </span>
                  </li>
                ))}
              </ol>
            </div>
            <div className="card insight">
              <h2>{t("storage.emptyFolders")}</h2>
              {empty === null ? (
                <span className="muted">{t("storage.looking")}</span>
              ) : empty.length === 0 ? (
                <span className="muted">{t("storage.none")}</span>
              ) : (
                <ol>
                  {empty.slice(0, 20).map((p) => (
                    <li key={p}>
                      <span className="insight-label" title={p}>{p}</span>
                    </li>
                  ))}
                  {empty.length > 20 && (
                    <li><span className="muted">{t("storage.andMore", { n: empty.length - 20 })}</span></li>
                  )}
                </ol>
              )}
            </div>
          </div>
        </>
      )}
    </Screen>
  );
}
