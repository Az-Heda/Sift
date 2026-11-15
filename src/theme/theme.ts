export type ThemeMode = "system" | "light" | "dark";

export interface ThemeSettings {
  mode: ThemeMode;
  primary: string;
  secondary: string;
  /** 0–100 */
  glow: number;
  /** corner radius in px */
  radius: number;
}

export const DEFAULT_THEME: ThemeSettings = {
  mode: "system",
  primary: "#3b82f6",
  secondary: "#8b5cf6",
  glow: 50,
  radius: 10,
};

export const PRESETS: { name: string; primary: string; secondary: string }[] = [
  { name: "Ion", primary: "#3b82f6", secondary: "#8b5cf6" },
  { name: "Neon", primary: "#00b8d9", secondary: "#e11d9a" },
  { name: "Solar", primary: "#f59e0b", secondary: "#ef4444" },
  { name: "Matrix", primary: "#16a34a", secondary: "#06b6d4" },
];

const STORAGE_KEY = "sift.theme";
const HEX = /^#[0-9a-f]{6}$/i;

export function loadTheme(): ThemeSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_THEME;
    const p = JSON.parse(raw) as Partial<ThemeSettings>;
    return {
      mode: p.mode === "light" || p.mode === "dark" ? p.mode : "system",
      primary: HEX.test(p.primary ?? "") ? p.primary! : DEFAULT_THEME.primary,
      secondary: HEX.test(p.secondary ?? "") ? p.secondary! : DEFAULT_THEME.secondary,
      glow: clamp(p.glow, 0, 100, DEFAULT_THEME.glow),
      radius: clamp(p.radius, 0, 24, DEFAULT_THEME.radius),
    };
  } catch {
    return DEFAULT_THEME;
  }
}

export function saveTheme(t: ThemeSettings) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(t));
  } catch {
    /* storage unavailable: theme just won't persist */
  }
}

function clamp(v: unknown, min: number, max: number, fallback: number) {
  return typeof v === "number" && Number.isFinite(v)
    ? Math.min(max, Math.max(min, v))
    : fallback;
}

/** Pick black or white text for readable contrast on the given background. */
function onColor(hex: string): string {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  const lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  return lum > 0.4 ? "#0a0d1a" : "#ffffff";
}

export function systemPrefersDark(): boolean {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function applyTheme(t: ThemeSettings) {
  const root = document.documentElement;
  const dark = t.mode === "system" ? systemPrefersDark() : t.mode === "dark";
  root.dataset.theme = dark ? "dark" : "light";
  root.style.setProperty("--primary", t.primary);
  root.style.setProperty("--secondary", t.secondary);
  root.style.setProperty("--on-primary", onColor(t.primary));
  root.style.setProperty("--glow", String(t.glow / 100));
  root.style.setProperty("--radius", `${t.radius}px`);
}
