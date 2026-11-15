/** Image formats Sift scans. Each entry groups extensions that mean the same format. */
export const FILE_TYPES = [
  { id: "jpeg", label: "JPEG", exts: ["jpg", "jpeg"] },
  { id: "png", label: "PNG", exts: ["png"] },
  { id: "webp", label: "WEBP", exts: ["webp"] },
  { id: "gif", label: "GIF", exts: ["gif"] },
  { id: "tiff", label: "TIFF", exts: ["tif", "tiff"] },
] as const;

export const ALL_EXTS: string[] = FILE_TYPES.flatMap((t) => [...t.exts]);

export const extOf = (path: string) => {
  const i = path.lastIndexOf(".");
  return i < 0 ? "" : path.slice(i + 1).toLowerCase();
};
