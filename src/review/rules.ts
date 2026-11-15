import type { DupFile, DupGroup } from "../api";
import type { MsgKey } from "../i18n";
import { isInside } from "./filters";

export type KeepRule = "recommended" | "oldest" | "newest" | "master";

export const RULES: { id: KeepRule; label: MsgKey }[] = [
  { id: "recommended", label: "rule.recommended" },
  { id: "oldest", label: "rule.oldest" },
  { id: "newest", label: "rule.newest" },
  { id: "master", label: "rule.master" },
];

const pixels = (f: DupFile) => (f.width ?? 0) * (f.height ?? 0);
const age = (f: DupFile) => (f.created == null ? f.modified : Math.min(f.created, f.modified));
const byPath = (a: DupFile, b: DupFile) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0);

/** Same order the backend uses for the recommended copy: resolution, then age, then path. */
const best = (files: DupFile[]) =>
  [...files].sort((a, b) => pixels(b) - pixels(a) || age(a) - age(b) || byPath(a, b))[0];

/** The copy a rule would keep, or null when the rule doesn't apply to this group. */
export function pickKeeper(g: DupGroup, rule: KeepRule, master: string): DupFile | null {
  switch (rule) {
    case "recommended":
      return g.files.find((f) => f.id === g.keep_id) ?? best(g.files);
    case "oldest":
      return [...g.files].sort((a, b) => age(a) - age(b) || byPath(a, b))[0];
    case "newest":
      return [...g.files].sort((a, b) => b.modified - a.modified || byPath(a, b))[0];
    case "master": {
      if (!master) return null;
      const inMaster = g.files.filter((f) => isInside(f.path, master));
      return inMaster.length ? best(inMaster) : null;
    }
  }
}

/** Ids to select (every copy except the keeper) for each group, plus how many groups were skipped. */
export function applyRule(groups: DupGroup[], rule: KeepRule, master: string) {
  const ids: number[] = [];
  let skipped = 0;
  for (const g of groups) {
    const keeper = pickKeeper(g, rule, master);
    if (!keeper) {
      skipped++;
      continue;
    }
    for (const f of g.files) if (f.id !== keeper.id) ids.push(f.id);
  }
  return { ids, skipped };
}
