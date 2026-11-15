import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import type { DupGroup } from "../api";
import { useScan } from "../scan/ScanProvider";
import { DEFAULT_FILTERS, type Filters } from "./filters";

interface ReviewCtx {
  /** Ids of files the user has marked for cleanup. */
  selected: ReadonlySet<number>;
  toggle: (id: number) => void;
  add: (ids: number[]) => void;
  remove: (ids: number[]) => void;
  clear: () => void;
  /** Selects every copy in `group` except `keepId`. */
  keepOnly: (group: DupGroup, keepId: number) => void;
  filters: Filters;
  setFilters: (patch: Partial<Filters>) => void;
  resetFilters: () => void;
  compare: DupGroup | null;
  openCompare: (group: DupGroup) => void;
}

const Ctx = createContext<ReviewCtx | null>(null);

/** Selection, filters and the Compare target live above the screens so they survive navigation. */
export function ReviewProvider({
  children,
  onCompare,
}: {
  children: ReactNode;
  onCompare: () => void;
}) {
  const { version } = useScan();
  const [selected, setSelected] = useState<ReadonlySet<number>>(new Set());
  const [filters, setFiltersState] = useState<Filters>(DEFAULT_FILTERS);
  const [compare, setCompare] = useState<DupGroup | null>(null);

  // A finished scan can delete or renumber files, so ids from before it are stale.
  useEffect(() => {
    setSelected(new Set());
    setCompare(null);
  }, [version]);

  const add = useCallback((ids: number[]) => setSelected((s) => new Set([...s, ...ids])), []);
  const remove = useCallback(
    (ids: number[]) => setSelected((s) => new Set([...s].filter((i) => !ids.includes(i)))),
    [],
  );
  const toggle = useCallback(
    (id: number) =>
      setSelected((s) => {
        const n = new Set(s);
        if (!n.delete(id)) n.add(id);
        return n;
      }),
    [],
  );
  const clear = useCallback(() => setSelected(new Set()), []);
  const keepOnly = useCallback((group: DupGroup, keepId: number) => {
    const others = group.files.filter((f) => f.id !== keepId).map((f) => f.id);
    setSelected((s) => new Set([...[...s].filter((i) => i !== keepId), ...others]));
  }, []);
  const setFilters = useCallback(
    (patch: Partial<Filters>) => setFiltersState((f) => ({ ...f, ...patch })),
    [],
  );
  const resetFilters = useCallback(() => setFiltersState(DEFAULT_FILTERS), []);
  const openCompare = useCallback(
    (group: DupGroup) => {
      setCompare(group);
      onCompare();
    },
    [onCompare],
  );

  const value = useMemo(
    () => ({
      selected, toggle, add, remove, clear, keepOnly,
      filters, setFilters, resetFilters, compare, openCompare,
    }),
    [selected, toggle, add, remove, clear, keepOnly, filters, setFilters, resetFilters, compare, openCompare],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useReview(): ReviewCtx {
  const c = useContext(Ctx);
  if (!c) throw new Error("useReview must be used inside <ReviewProvider>");
  return c;
}
