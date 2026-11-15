import { useEffect, useMemo, useState } from "react";
import {
  listDuplicates,
  listFolders,
  openContainingFolder,
  revealFile,
  type DupGroup,
  type Folder,
} from "../api";
import CopyCard from "../components/CopyCard";
import FilterBar from "../components/FilterBar";
import { formatBytes } from "../format";
import { applyFilters, isFiltered } from "../review/filters";
import { useReview } from "../review/ReviewProvider";
import { useScan } from "../scan/ScanProvider";
import { Empty, Screen } from "./Placeholder";

const PAGE = 25;
const errMsg = (e: unknown) => (typeof e === "string" ? e : (e as Error)?.message ?? "Unknown error");

export default function Duplicates() {
  const { version } = useScan();
  const review = useReview();
  const { selected, filters } = review;
  const [groups, setGroups] = useState<DupGroup[] | null>(null);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [shown, setShown] = useState(PAGE);

  useEffect(() => {
    listDuplicates()
      .then((g) => {
        setGroups(g);
        setError(null);
      })
      .catch((e) => setError(errMsg(e)));
    listFolders().then(setFolders).catch(() => setFolders([]));
  }, [version]);

  const visible = useMemo(() => (groups ? applyFilters(groups, filters) : []), [groups, filters]);

  // Restart paging whenever the filtered list changes.
  useEffect(() => setShown(PAGE), [visible]);

  const sizeById = useMemo(() => {
    const m = new Map<number, number>();
    groups?.forEach((g) => g.files.forEach((f) => m.set(f.id, g.size)));
    return m;
  }, [groups]);
  const selectedBytes = useMemo(
    () => [...selected].reduce((n, id) => n + (sizeById.get(id) ?? 0), 0),
    [selected, sizeById],
  );

  const act = (fn: (id: number) => Promise<void>) => (id: number) =>
    fn(id).then(() => setError(null), (e) => setError(errMsg(e)));

  function selectAllButRecommended() {
    review.add(visible.flatMap((g) => g.files.filter((f) => f.id !== g.keep_id).map((f) => f.id)));
  }

  return (
    <Screen title="Duplicates" subtitle="Groups of byte-identical files, largest savings first.">
      {error && <div className="alert" role="alert">{error}</div>}
      {groups === null ? null : groups.length === 0 ? (
        <Empty text="No duplicates found. Run a scan from the Dashboard or Folders screen." />
      ) : (
        <>
          <FilterBar
            filters={filters}
            folders={folders}
            onChange={review.setFilters}
            onReset={review.resetFilters}
          />
          <div className="toolbar">
            <button className="btn" onClick={selectAllButRecommended} disabled={visible.length === 0}>
              Select all except recommended
            </button>
            <button className="btn" onClick={review.clear} disabled={selected.size === 0}>
              Clear selection
            </button>
            <span className="muted" aria-live="polite">
              {selected.size > 0
                ? `${selected.size} selected · ${formatBytes(selectedBytes)}`
                : `${visible.length} of ${groups.length} groups`}
            </span>
          </div>

          {visible.length === 0 ? (
            <Empty text={isFiltered(filters) ? "No groups match these filters." : "Nothing to show."} />
          ) : (
            <ul className="group-list">
              {visible.slice(0, shown).map((g) => {
                const allSelected = g.files.every((f) => selected.has(f.id));
                return (
                  <li className="card group" key={g.sha256}>
                    <div className="group-head">
                      <strong>{g.files.length} copies</strong>
                      <span className="muted">
                        {formatBytes(g.size)} each · {formatBytes(g.wasted)} recoverable
                      </span>
                    </div>
                    {allSelected && (
                      <div className="warn" role="status">
                        Every copy in this group is selected: nothing would be kept.
                      </div>
                    )}
                    <div className="copies">
                      {g.files.map((f) => (
                        <CopyCard
                          key={f.id}
                          file={f}
                          size={g.size}
                          recommended={f.id === g.keep_id}
                          selected={selected.has(f.id)}
                          onToggle={() => review.toggle(f.id)}
                          onReveal={() => act(revealFile)(f.id)}
                          onOpenFolder={() => act(openContainingFolder)(f.id)}
                        />
                      ))}
                    </div>
                    <div className="group-actions">
                      <button className="btn small" onClick={() => review.keepOnly(g, g.keep_id)}>
                        Keep recommended
                      </button>
                      <button className="btn small primary" onClick={() => review.openCompare(g)}>
                        Compare
                      </button>
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
          {shown < visible.length && (
            <div className="toolbar">
              <button className="btn" onClick={() => setShown((n) => n + PAGE)}>
                Show more ({visible.length - shown} remaining)
              </button>
            </div>
          )}
        </>
      )}
    </Screen>
  );
}
