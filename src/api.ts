import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

export interface Folder {
  id: number;
  path: string;
  added_at: number;
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
