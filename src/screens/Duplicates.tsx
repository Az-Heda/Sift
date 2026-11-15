import { useEffect, useState } from "react";
import { listDuplicates, type DupGroup } from "../api";
import { formatBytes } from "../format";
import { useScan } from "../scan/ScanProvider";
import { Empty, Screen } from "./Placeholder";

export default function Duplicates() {
  const { version } = useScan();
  const [groups, setGroups] = useState<DupGroup[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    listDuplicates()
      .then((g) => {
        setGroups(g);
        setError(null);
      })
      .catch((e) => setError(String(e)));
  }, [version]);

  return (
    <Screen title="Duplicates" subtitle="Groups of byte-identical files, largest savings first.">
      {error && <div className="alert" role="alert">{error}</div>}
      {groups === null ? null : groups.length === 0 ? (
        <Empty text="No duplicates found. Run a scan from the Dashboard or Folders screen." />
      ) : (
        <ul className="group-list">
          {groups.map((g) => (
            <li className="card group" key={g.sha256}>
              <div className="group-head">
                <strong>{g.files.length} copies</strong>
                <span className="muted">
                  {formatBytes(g.size)} each · {formatBytes(g.wasted)} recoverable
                </span>
              </div>
              <ul className="group-files">
                {g.files.map((f) => (
                  <li key={f.id} className="folder-path" title={f.path}>{f.path}</li>
                ))}
              </ul>
            </li>
          ))}
        </ul>
      )}
    </Screen>
  );
}
