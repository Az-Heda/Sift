import { useState } from "react";
import { thumbUrl, type DupFile } from "../api";
import { useI18n } from "../i18n";
import { formatBytes, formatDate, splitPath } from "../format";

export default function CopyCard({
  file,
  recommended,
  selected,
  onToggle,
  onReveal,
  onOpenFolder,
}: {
  file: DupFile;
  recommended: boolean;
  selected: boolean;
  onToggle: () => void;
  onReveal: () => void;
  onOpenFolder: () => void;
}) {
  const { t } = useI18n();
  const [failed, setFailed] = useState(false);
  const [folder, name] = splitPath(file.path);
  const hasDims = !!file.width && !!file.height;
  const cls = ["copy", recommended && "recommended", selected && "selected"].filter(Boolean).join(" ");

  return (
    <article className={cls}>
      <div className="thumb">
        {failed ? (
          <span className="thumb-fallback">{t("card.noPreview")}</span>
        ) : (
          <img
            src={thumbUrl(file.id)}
            alt={name}
            loading="lazy"
            decoding="async"
            onError={() => setFailed(true)}
          />
        )}
        {recommended && <span className="keep-badge">{t("common.recommended")}</span>}
        <label className="pick" title={t("card.select")}>
          <input
            type="checkbox"
            checked={selected}
            onChange={onToggle}
            aria-label={t("card.selectAria", { name })}
          />
        </label>
      </div>
      <div className="copy-info">
        <span className="copy-name" title={name}>{name}</span>
        <span className="copy-folder" title={folder}>{folder}</span>
        <dl className="copy-meta">
          <dt>{t("common.resolution")}</dt>
          <dd>{hasDims ? `${file.width} × ${file.height}` : t("common.unknown")}</dd>
          <dt>{t("common.size")}</dt>
          <dd>{formatBytes(file.size)}</dd>
          <dt>{t("common.modified")}</dt>
          <dd>{formatDate(file.modified)}</dd>
          <dt>{t("common.created")}</dt>
          <dd>{file.created ? formatDate(file.created) : t("common.unknown")}</dd>
        </dl>
        <div className="copy-actions">
          <button className="btn small" onClick={onReveal} title={t("card.revealTitle")}>
            {t("common.reveal")}
          </button>
          <button className="btn small" onClick={onOpenFolder} title={t("card.openFolderTitle")}>
            {t("common.openFolder")}
          </button>
        </div>
      </div>
    </article>
  );
}
