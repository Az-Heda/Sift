import { Archive, Film, File, FileText, Image, Music } from "lucide-react";
import { categoryOf, extOf, type Category } from "../review/types";

const ICONS: Record<Category, typeof File> = {
  image: Image,
  video: Film,
  audio: Music,
  document: FileText,
  archive: Archive,
};

/** Stand-in for a preview when a file isn't an image: a category icon and its extension. */
export default function FileGlyph({ path, size = 40 }: { path: string; size?: number }) {
  const cat = categoryOf(path);
  const Icon = cat ? ICONS[cat] : File;
  return (
    <span className="glyph" aria-hidden="true">
      <Icon size={size} strokeWidth={1.5} />
      <span className="glyph-ext">{extOf(path).toUpperCase()}</span>
    </span>
  );
}
