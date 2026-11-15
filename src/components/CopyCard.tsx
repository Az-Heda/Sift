import { useState } from "react";
import { thumbUrl, type DupFile } from "../api";
import { formatBytes, formatDate, splitPath } from "../format";

export default function CopyCard({
  file,
  size,
  recommended,
  selected,
  onToggle,
  onReveal,
  onOpenFolder,
}: {
  file: DupFile;
  size: number;
  recommended: boolean;
  selected: boolean;
  onToggle: () => void;
  onReveal: () => void;
  onOpenFolder: () => void;
}) {
  const [failed, setFailed] = useState(false);
  const [folder, name] = splitPath(file.path);
  const hasDims = !!file.width && !!file.height;
  const cls = ["copy", recommended && "recommended", selected && "selected"].filter(Boolean).join(" ");

  return (
    <article className={cls}>
      <div className="thumb">
        {failed ? (
          <span className="thumb-fallback">No preview</span>
        ) : (
          <img
            src={thumbUrl(file.id)}
            alt={name}
            loading="lazy"
            decoding="async"
            onError={() => setFailed(true)}
          />
        )}
        {recommended && <span className="keep-badge">Recommended</span>}
        <label className="pick" title="Select for cleanup">
          <input
            type="checkbox"
            checked={selected}
            onChange={onToggle}
            aria-label={`Select ${name}`}
          />
        </label>
      </div>
      <div className="copy-info">
        <span className="copy-name" title={name}>{name}</span>
        <span className="copy-folder" title={folder}>{folder}</span>
        <dl className="copy-meta">
          <dt>Resolution</dt>
          <dd>{hasDims ? `${file.width} × ${file.height}` : "Unknown"}</dd>
          <dt>Size</dt>
          <dd>{formatBytes(size)}</dd>
          <dt>Modified</dt>
          <dd>{formatDate(file.modified)}</dd>
          <dt>Created</dt>
          <dd>{file.created ? formatDate(file.created) : "Unknown"}</dd>
        </dl>
        <div className="copy-actions">
          <button className="btn small" onClick={onReveal} title="Show this file in your file manager">
            Reveal
          </button>
          <button className="btn small" onClick={onOpenFolder} title="Open the containing folder">
            Open folder
          </button>
        </div>
      </div>
    </article>
  );
}
