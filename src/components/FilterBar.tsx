import type { Folder } from "../api";
import { splitPath } from "../format";
import { useI18n, type MsgKey } from "../i18n";
import { isFiltered, type Filters, type SortKey } from "../review/filters";
import { FILE_TYPES } from "../review/types";

const SORTS: { id: SortKey; label: MsgKey }[] = [
  { id: "savings", label: "filter.sort.savings" },
  { id: "copies", label: "filter.sort.copies" },
  { id: "size", label: "filter.sort.size" },
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
  const { t } = useI18n();
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
          placeholder={t("filter.searchPlaceholder")}
          aria-label={t("filter.searchAria")}
          value={filters.search}
          onChange={(e) => onChange({ search: e.target.value })}
        />
        <label className="inline">
          <span className="field-label">{t("filter.sort")}</span>
          <select
            className="input"
            value={filters.sort}
            onChange={(e) => onChange({ sort: e.target.value as SortKey })}
          >
            {SORTS.map((s) => (
              <option key={s.id} value={s.id}>{t(s.label)}</option>
            ))}
          </select>
        </label>
      </div>

      <div className="filter-row">
        <div className="chips" role="group" aria-label={t("filter.fileType")}>
          {FILE_TYPES.map((ft) => (
            <button
              key={ft.id}
              className={filters.types.includes(ft.id) ? "chip active" : "chip"}
              aria-pressed={filters.types.includes(ft.id)}
              onClick={() => toggleType(ft.id)}
            >
              {ft.labelKey ? t(ft.labelKey) : ft.label}
            </button>
          ))}
        </div>
        <label className="inline">
          <span className="field-label">{t("filter.sizeMb")}</span>
          <input
            type="number" min={0} step="any" className="input num" placeholder={t("filter.min")}
            aria-label={t("filter.minAria")}
            value={filters.minMb}
            onChange={(e) => onChange({ minMb: e.target.value })}
          />
          <input
            type="number" min={0} step="any" className="input num" placeholder={t("filter.max")}
            aria-label={t("filter.maxAria")}
            value={filters.maxMb}
            onChange={(e) => onChange({ maxMb: e.target.value })}
          />
        </label>
      </div>

      <div className="filter-row">
        <label className="inline">
          <span className="field-label">{t("filter.folder")}</span>
          <select
            className="input"
            value={filters.folder}
            onChange={(e) => onChange({ folder: e.target.value })}
          >
            <option value="">{t("filter.allFolders")}</option>
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
          <span>{t("filter.cross")}</span>
        </label>
        {isFiltered(filters) && (
          <button className="btn small" onClick={onReset}>{t("filter.clear")}</button>
        )}
      </div>
    </div>
  );
}
