import { useEffect, useState } from "react";
import { listDuplicates, type DupGroup } from "../api";
import CopyCard from "../components/CopyCard";
import { formatBytes } from "../format";
import { useScan } from "../scan/ScanProvider";
import { Empty, Screen } from "./Placeholder";

const PAGE = 25;

export default function Duplicates() {
  const { version } = useScan();
  const [groups, setGroups] = useState<DupGroup[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [shown, setShown] = useState(PAGE);

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
        <>
          <ul className="group-list">
            {groups.slice(0, shown).map((g) => (
              <li className="card group" key={g.sha256}>
                <div className="group-head">
                  <strong>{g.files.length} copies</strong>
                  <span className="muted">
                    {formatBytes(g.size)} each · {formatBytes(g.wasted)} recoverable
                  </span>
                </div>
                <div className="copies">
                  {g.files.map((f) => (
                    <CopyCard
                      key={f.id}
                      file={f}
                      size={g.size}
                      recommended={f.id === g.keep_id}
                    />
                  ))}
                </div>
              </li>
            ))}
          </ul>
          {shown < groups.length && (
            <div className="toolbar">
              <button className="btn" onClick={() => setShown((n) => n + PAGE)}>
                Show more ({groups.length - shown} remaining)
              </button>
            </div>
          )}
        </>
      )}
    </Screen>
  );
}
