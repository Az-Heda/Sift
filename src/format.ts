let locale: string | undefined;

/** Called by the language provider so numbers and dates follow the app language. */
export const setLocale = (l: string | undefined) => {
  locale = l;
};

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  const digits = v >= 100 ? 0 : 1;
  const text = v.toLocaleString(locale, { minimumFractionDigits: digits, maximumFractionDigits: digits });
  return `${text} ${units[i]}`;
}

export const formatNumber = (n: number) => n.toLocaleString(locale);

/** Splits a path into [folder, filename], handling both / and \ separators. */
export function splitPath(path: string): [string, string] {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i < 0 ? ["", path] : [path.slice(0, i), path.slice(i + 1)];
}

export const formatDate = (unixSeconds: number) =>
  new Date(unixSeconds * 1000).toLocaleDateString(locale, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });

export const formatDateTime = (unixSeconds: number) =>
  new Date(unixSeconds * 1000).toLocaleString(locale);
