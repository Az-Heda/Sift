import type { Folder } from "../api";
import { splitPath } from "../format";
import { isFiltered, type Filters, type SortKey } from "../review/filters";
import { FILE_TYPES } from "../review/types";

const SORTS: { id: SortKey; label: string }[] = [
  { id: "savings", label: "Largest savings" },
  { id: "copies", label: "Most copies" },
  { id: "size", label: "Largest file" },
];

export default function FilterBar({
  filters,
  folders,
  onChange,
  onReset,
}: {
  filters: Filters;
  folders: Folder[];
  onChange: (patch: Partial<Filters>) => void;
  onReset: () => void;
}) {
  const toggleType = (id: string) =>
    onChange({
      types: filters.types.includes(id)
        ? filters.types.filter((t) => t !== id)
        : [...filters.types, id],
    });

  return (
    <div className="card filters">
      <div className="filter-row">
        <input
          type="search"
          className="input grow"
          placeholder="Search by filename…"
          aria-label="Search by filename"
          value={filters.search}
          onChange={(e) => onChange({ search: e.target.value })}
        />
        <label className="inline">
          <span className="field-label">Sort</span>
          <select
            className="input"
            value={filters.sort}
            onChange={(e) => onChange({ sort: e.target.value as SortKey })}
          >
            {SORTS.map((s) => (
              <option key={s.id} value={s.id}>{s.label}</option>
            ))}
          </select>
        </label>
      </div>

      <div className="filter-row">
        <div className="chips" role="group" aria-label="File type">
          {FILE_TYPES.map((t) => (
            <button
              key={t.id}
              className={filters.types.includes(t.id) ? "chip active" : "chip"}
              aria-pressed={filters.types.includes(t.id)}
              onClick={() => toggleType(t.id)}
            >
              {t.label}
            </button>
          ))}
        </div>
        <label className="inline">
          <span className="field-label">Size (MB)</span>
          <input
            type="number" min={0} step="any" className="input num" placeholder="min"
            aria-label="Minimum size in MB"
            value={filters.minMb}
            onChange={(e) => onChange({ minMb: e.target.value })}
          />
          <input
            type="number" min={0} step="any" className="input num" placeholder="max"
            aria-label="Maximum size in MB"
            value={filters.maxMb}
            onChange={(e) => onChange({ maxMb: e.target.value })}
          />
        </label>
      </div>

      <div className="filter-row">
        <label className="inline">
          <span className="field-label">Folder</span>
          <select
            className="input"
            value={filters.folder}
            onChange={(e) => onChange({ folder: e.target.value })}
          >
            <option value="">All folders</option>
            {folders.map((f) => (
              <option key={f.id} value={f.path}>{splitPath(f.path)[1] || f.path}</option>
            ))}
          </select>
        </label>
        <label className="inline check">
          <input
            type="checkbox"
            checked={filters.crossFolderOnly}
            onChange={(e) => onChange({ crossFolderOnly: e.target.checked })}
          />
          <span>Only duplicates across different folders</span>
        </label>
        {isFiltered(filters) && (
          <button className="btn small" onClick={onReset}>Clear filters</button>
        )}
      </div>
    </div>
  );
}
