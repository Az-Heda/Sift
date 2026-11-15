import { invoke } from "@tauri-apps/api/core";
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
  phase: "discovering" | "hashing" | "finalizing";
  files_found: number;
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
  errors: number;
}

export interface ScanStatus {
  running: boolean;
  progress: ScanProgress | null;
}

export interface DupFile {
  id: number;
  path: string;
}

export interface DupGroup {
  sha256: string;
  size: number;
  files: DupFile[];
  wasted: number;
}

export interface Stats {
  files: number;
  bytes: number;
  groups: number;
  wasted: number;
  last_scan_at: number | null;
}

export const startScan = () => invoke<void>("start_scan");
export const cancelScan = () => invoke<void>("cancel_scan");
export const scanStatus = () => invoke<ScanStatus>("scan_status");
export const listDuplicates = () => invoke<DupGroup[]>("list_duplicates");
export const getStats = () => invoke<Stats>("get_stats");
