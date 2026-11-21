import { useEffect, useState } from "react";
import {
  fullUrl,
  openContainingFolder,
  revealFile,
  type DupFile,
  type DupGroup,
} from "../api";
import { formatBytes, formatDate, splitPath } from "../format";
import { useI18n } from "../i18n";
import { useReview } from "../review/ReviewProvider";
import { Empty, Screen } from "./Placeholder";

const errMsg = (e: unknown) => (typeof e === "string" ? e : (e as Error)?.message ?? "Unknown error");

function Pane({
  group,
  file,
  other,
  files,
  zoom,
  onPick,
  onKeep,
  onError,
}: {
  group: DupGroup;
  file: DupFile;
  other: DupFile;
  files: DupFile[];
  zoom: boolean;
  onPick: (id: number) => void;
  onKeep: () => void;
  onError: (m: string) => void;
}) {
  const { t } = useI18n();
  const { selected, toggle } = useReview();
  const [failed, setFailed] = useState(false);
  const [folder, name] = splitPath(file.path);
  const hasDims = !!file.width && !!file.height;
  const run = (fn: (id: number) => Promise<void>) => fn(file.id).catch((e) => onError(errMsg(e)));

  useEffect(() => setFailed(false), [file.id]);

  return (
    <section className={file.id === group.keep_id ? "pane recommended" : "pane"}>
      <label className="inline">
        <span className="field-label">{t("compare.copy")}</span>
        <select className="input grow" value={file.id} onChange={(e) => onPick(Number(e.target.value))}>
          {files.map((f) => (
            <option key={f.id} value={f.id} disabled={f.id === other.id}>
              {splitPath(f.path)[1]} — {splitPath(f.path)[0]}
            </option>
          ))}
        </select>
      </label>

      <div className={zoom ? "viewer zoomed" : "viewer"}>
        {failed ? (
          <span className="thumb-fallback">{t("compare.cantDisplay")}</span>
        ) : (
          <img src={fullUrl(file.id)} alt={name} decoding="async" onError={() => setFailed(true)} />
        )}
        {file.id === group.keep_id && <span className="keep-badge">{t("common.recommended")}</span>}
      </div>

      <dl className="copy-meta">
        <dt>{t("compare.name")}</dt><dd title={name}>{name}</dd>
        <dt>{t("compare.location")}</dt><dd title={folder}>{folder}</dd>
        <dt>{t("common.resolution")}</dt><dd>{hasDims ? `${file.width} × ${file.height}` : t("common.unknown")}</dd>
        <dt>{t("common.size")}</dt><dd>{formatBytes(file.size)}</dd>
        <dt>{t("common.modified")}</dt><dd>{formatDate(file.modified)}</dd>
        <dt>{t("common.created")}</dt><dd>{file.created ? formatDate(file.created) : t("common.unknown")}</dd>
      </dl>

      <div className="copy-actions">
        <button className="btn small primary" onClick={onKeep}>{t("compare.keepThis")}</button>
        <label className="inline check">
          <input type="checkbox" checked={selected.has(file.id)} onChange={() => toggle(file.id)} />
          <span>{t("compare.selectForCleanup")}</span>
        </label>
        <button className="btn small" onClick={() => run(revealFile)}>{t("common.reveal")}</button>
        <button className="btn small" onClick={() => run(openContainingFolder)}>{t("common.openFolder")}</button>
      </div>
    </section>
  );
}

export default function Compare() {
  const { t } = useI18n();
  const { compare: group, keepOnly } = useReview();
  const [leftId, setLeftId] = useState(0);
  const [rightId, setRightId] = useState(0);
  const [zoom, setZoom] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!group) return;
    const keep = group.files.find((f) => f.id === group.keep_id) ?? group.files[0];
    setLeftId(keep.id);
    setRightId((group.files.find((f) => f.id !== keep.id) ?? keep).id);
    setError(null);
  }, [group]);

  if (!group) {
    return (
      <Screen title={t("compare.title")} subtitle={t("compare.subtitleEmpty")}>
        <Empty text={t("compare.empty")} />
      </Screen>
    );
  }

  const left = group.files.find((f) => f.id === leftId) ?? group.files[0];
  const right = group.files.find((f) => f.id === rightId) ?? group.files[1] ?? group.files[0];
  const pick = (side: "l" | "r") => (id: number) => (side === "l" ? setLeftId(id) : setRightId(id));

  return (
    <Screen
      title={t("compare.title")}
      subtitle={
        group.kind === "similar"
          ? t("compare.subtitleSimilar", { n: group.files.length })
          : t("compare.subtitle", { n: group.files.length, size: formatBytes(group.size) })
      }
    >
      {error && <div className="alert" role="alert">{error}</div>}
      <div className="toolbar">
        <button
          className="btn"
          onClick={() => {
            setLeftId(rightId);
            setRightId(leftId);
          }}
        >
          {t("compare.swap")}
        </button>
        <button className="btn" aria-pressed={zoom} onClick={() => setZoom((z) => !z)}>
          {zoom ? t("compare.fit") : t("compare.actual")}
        </button>
        <span className="muted">
          {t(group.kind === "similar" ? "compare.hintSimilar" : "compare.hint")}
        </span>
      </div>
      <div className="compare">
        <Pane group={group} file={left} other={right} files={group.files} zoom={zoom}
          onPick={pick("l")} onKeep={() => keepOnly(group, left.id)} onError={setError} />
        <Pane group={group} file={right} other={left} files={group.files} zoom={zoom}
          onPick={pick("r")} onKeep={() => keepOnly(group, right.id)} onError={setError} />
      </div>
    </Screen>
  );
}
