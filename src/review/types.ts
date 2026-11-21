import type { MsgKey } from "../i18n";

export type Category = "image" | "video" | "audio" | "document" | "archive";

/**
 * File types Sift scans. Each entry groups extensions that mean the same thing to the user.
 * Keep the extensions in step with the lists in `src-tauri/src/scan.rs`.
 * Only images get previews, dimensions and similarity detection.
 */
export const FILE_TYPES: readonly {
  id: string;
  /** Shown as-is unless `labelKey` is set. */
  label: string;
  labelKey?: MsgKey;
  category: Category;
  exts: readonly string[];
}[] = [
  { id: "jpeg", label: "JPEG", category: "image", exts: ["jpg", "jpeg"] },
  { id: "png", label: "PNG", category: "image", exts: ["png"] },
  { id: "webp", label: "WEBP", category: "image", exts: ["webp"] },
  { id: "gif", label: "GIF", category: "image", exts: ["gif"] },
  { id: "tiff", label: "TIFF", category: "image", exts: ["tif", "tiff"] },
  {
    id: "video", label: "Video", labelKey: "type.video", category: "video",
    exts: ["mp4", "m4v", "mov", "mkv", "avi", "webm", "wmv", "flv", "mpg", "mpeg"],
  },
  {
    id: "audio", label: "Audio", labelKey: "type.audio", category: "audio",
    exts: ["mp3", "flac", "wav", "m4a", "aac", "ogg", "opus", "wma"],
  },
  { id: "pdf", label: "PDF", category: "document", exts: ["pdf"] },
  {
    id: "office", label: "Documents", labelKey: "type.documents", category: "document",
    exts: ["doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "epub"],
  },
  {
    id: "archive", label: "Archives", labelKey: "type.archives", category: "archive",
    exts: ["zip", "rar", "7z", "tar", "gz", "bz2", "xz"],
  },
];

export const ALL_EXTS: string[] = FILE_TYPES.flatMap((t) => [...t.exts]);

export const extOf = (path: string) => {
  const i = path.lastIndexOf(".");
  return i < 0 ? "" : path.slice(i + 1).toLowerCase();
};

export const categoryOf = (path: string): Category | null => {
  const ext = extOf(path);
  return FILE_TYPES.find((t) => t.exts.includes(ext))?.category ?? null;
};

/** Images are the only files that can be previewed. */
export const isImage = (path: string) => categoryOf(path) === "image";
