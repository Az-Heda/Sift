import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

export interface Folder {
  id: number;
  path: string;
  added_at: number;
  last_scanned_at: number | null;
  file_count: number;
  exists: boolean;
}

export const listFolders = () => invoke<Folder[]>("list_folders");
export const addFolder = (path: string) => invoke<Folder>("add_folder", { path });
export const removeFolder = (id: number) => invoke<void>("remove_folder", { id });
export const getSetting = (key: string) => invoke<string | null>("get_setting", { key });
export const setSetting = (key: string, value: string) =>
  invoke<void>("set_setting", { key, value });

/** Opens the native folder picker. Resolves to null if cancelled. */
export async function pickFolder(): Promise<string | null> {
  const result = await open({ directory: true, multiple: false });
  return typeof result === "string" ? result : null;
}

// ---------- Scanning ----------
export interface ScanProgress {
  phase: "discovering" | "hashing" | "fingerprinting" | "finalizing";
  files_found: number;
  /** Work items in the current phase (files to hash or fingerprint) and how many are done. */
  hash_total: number;
  hash_done: number;
  errors: number;
}

export interface ScanSummary {
  scan_id: number;
  status: "completed" | "cancelled";
  files_seen: number;
  files_hashed: number;
  files_cached: number;
  files_fingerprinted: number;
  errors: number;
}

export interface ScanStatus {
  running: boolean;
  progress: ScanProgress | null;
}

export interface DupFile {
  id: number;
  path: string;
  size: number;
  /** Unix seconds. */
  modified: number;
  created: number | null;
  /** 0 x 0 means the dimensions could not be read. */
  width: number | null;
  height: number | null;
}

/** "exact": byte-identical files. "similar": images that look alike (perceptual hash). */
export type DupMode = "exact" | "similar";

export interface DupGroup {
  kind: DupMode;
  /** Identifies the group (a content hash for exact groups). */
  sha256: string;
  /** Size of each copy (exact) or of the largest copy (similar). */
  size: number;
  files: DupFile[];
  wasted: number;
  keep_id: number;
}

export interface Stats {
  files: number;
  bytes: number;
  groups: number;
  wasted: number;
  /** Groups of visually similar (not byte-identical) images. */
  similar_groups: number;
  /** Bytes permanently freed by cleanup. */
  recovered: number;
  /** Bytes currently held in quarantine. */
  quarantined: number;
  last_scan_at: number | null;
}

export const startScan = () => invoke<void>("start_scan");
export const cancelScan = () => invoke<void>("cancel_scan");
export const scanStatus = () => invoke<ScanStatus>("scan_status");
export const listDuplicates = () => invoke<DupGroup[]>("list_duplicates");
export const listSimilar = () => invoke<DupGroup[]>("list_similar");
export const listGroups = (mode: DupMode) => (mode === "similar" ? listSimilar() : listDuplicates());
export const getStats = () => invoke<Stats>("get_stats");

/** URL of the cached thumbnail for a tracked file (served by the `thumb://` scheme). */
export const thumbUrl = (fileId: number) => convertFileSrc(String(fileId), "thumb");

/** URL of the original, full-resolution image for a tracked file. */
export const fullUrl = (fileId: number) => convertFileSrc(`full-${fileId}`, "thumb");

export interface Insight {
  label: string;
  count: number;
  wasted: number;
}

export interface Insights {
  folders: Insight[];
  types: Insight[];
}

export const getInsights = () => invoke<Insights>("get_insights");
export const revealFile = (id: number) => invoke<void>("reveal_file", { id });
export const openContainingFolder = (id: number) => invoke<void>("open_containing_folder", { id });

/** Reads a JSON-encoded string list from settings. */
export async function getList(key: string): Promise<string[] | null> {
  const raw = await getSetting(key);
  if (raw === null) return null;
  try {
    const v = JSON.parse(raw);
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : null;
  } catch {
    return null;
  }
}
export const setList = (key: string, list: string[]) => setSetting(key, JSON.stringify(list));

// ---------- Cleanup ----------
export interface CleanupResult {
  done: number;
  bytes: number;
  failed: { path: string; reason: string }[];
}

export interface QuarantineItem {
  id: number;
  original_path: string;
  size: number;
  moved_at: number;
  width: number | null;
  height: number | null;
  exists: boolean;
}

/** `mode` tells the backend what "another copy remains" means for the chosen files. */
export const quarantineFiles = (ids: number[], mode: DupMode = "exact") =>
  invoke<CleanupResult>("quarantine_files", { ids, mode });
export const deleteFiles = (ids: number[], mode: DupMode = "exact") =>
  invoke<CleanupResult>("delete_files", { ids, mode });
export const listQuarantine = () => invoke<QuarantineItem[]>("list_quarantine");
export const restoreQuarantined = (ids: number[]) =>
  invoke<CleanupResult>("restore_quarantined", { ids });
export const deleteQuarantined = (ids: number[]) =>
  invoke<CleanupResult>("delete_quarantined", { ids });

export interface QuarantineDir {
  path: string;
  default: string;
}
export const getQuarantineDir = () => invoke<QuarantineDir>("get_quarantine_dir");
/** An empty path restores the default location. */
export const setQuarantineDir = (path: string) => invoke<QuarantineDir>("set_quarantine_dir", { path });

/** Thumbnail of an item in quarantine. */
export const quarantineThumbUrl = (itemId: number) => convertFileSrc(`q-${itemId}`, "thumb");
