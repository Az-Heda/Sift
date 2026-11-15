import type { DupGroup } from "../api";
import { splitPath } from "../format";
import { FILE_TYPES, extOf } from "./types";

export type SortKey = "savings" | "copies" | "size";

export interface Filters {
  search: string;
  /** FILE_TYPES ids; empty means every type. */
  types: string[];
  /** Megabytes; empty string means unbounded. */
  minMb: string;
  maxMb: string;
  /** Root folder path; empty means every folder. */
  folder: string;
  crossFolderOnly: boolean;
  sort: SortKey;
}

export const DEFAULT_FILTERS: Filters = {
  search: "",
  types: [],
  minMb: "",
  maxMb: "",
  folder: "",
  crossFolderOnly: false,
  sort: "savings",
};

export const isFiltered = (f: Filters) =>
  f.search !== "" || f.types.length > 0 || f.minMb !== "" || f.maxMb !== "" ||
  f.folder !== "" || f.crossFolderOnly;

const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");

/** True when `path` is inside `root` (component-wise, so /a/b doesn't match /a/bc). */
export function isInside(path: string, root: string): boolean {
  const p = norm(path);
  const r = norm(root);
  return p === r || p.startsWith(r + "/");
}

export function applyFilters(groups: DupGroup[], f: Filters): DupGroup[] {
  const q = f.search.trim().toLowerCase();
  const exts = new Set<string>(
    FILE_TYPES.filter((t) => f.types.includes(t.id)).flatMap((t) => [...t.exts]),
  );
  const min = f.minMb === "" ? null : Number(f.minMb) * 1024 * 1024;
  const max = f.maxMb === "" ? null : Number(f.maxMb) * 1024 * 1024;

  const out = groups.filter((g) => {
    if (min !== null && !Number.isNaN(min) && g.size < min) return false;
    if (max !== null && !Number.isNaN(max) && g.size > max) return false;
    if (q && !g.files.some((x) => splitPath(x.path)[1].toLowerCase().includes(q))) return false;
    if (exts.size && !g.files.some((x) => exts.has(extOf(x.path)))) return false;
    if (f.folder && !g.files.some((x) => isInside(x.path, f.folder))) return false;
    if (f.crossFolderOnly && new Set(g.files.map((x) => norm(splitPath(x.path)[0]))).size < 2) {
      return false;
    }
    return true;
  });

  const by: Record<SortKey, (a: DupGroup, b: DupGroup) => number> = {
    savings: (a, b) => b.wasted - a.wasted,
    copies: (a, b) => b.files.length - a.files.length || b.wasted - a.wasted,
    size: (a, b) => b.size - a.size || b.wasted - a.wasted,
  };
  return out.sort(by[f.sort]);
}
