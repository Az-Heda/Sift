import { useState } from "react";
import { thumbUrl, type DupFile } from "../api";
import { formatBytes, formatDate, splitPath } from "../format";

export default function CopyCard({
  file,
  size,
  recommended,
}: {
  file: DupFile;
  size: number;
  recommended: boolean;
}) {
  const [failed, setFailed] = useState(false);
  const [folder, name] = splitPath(file.path);
  const hasDims = !!file.width && !!file.height;

  return (
    <article className={recommended ? "copy recommended" : "copy"}>
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
      </div>
    </article>
  );
}
