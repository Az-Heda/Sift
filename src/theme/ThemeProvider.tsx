import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  applyTheme,
  DEFAULT_THEME,
  saveTheme,
  type ThemeSettings,
} from "./theme";

interface ThemeCtx {
  theme: ThemeSettings;
  update: (patch: Partial<ThemeSettings>) => void;
  reset: () => void;
}

const Ctx = createContext<ThemeCtx | null>(null);

export function ThemeProvider({
  initial,
  children,
}: {
  initial: ThemeSettings;
  children: ReactNode;
}) {
  const [theme, setTheme] = useState<ThemeSettings>(initial);

  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  // Persist to the database, debounced so dragging a slider doesn't spam writes.
  const first = useRef(true);
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    const t = setTimeout(() => void saveTheme(theme), 300);
    return () => clearTimeout(t);
  }, [theme]);

  // Follow OS changes while in "system" mode.
  useEffect(() => {
    if (theme.mode !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const on = () => applyTheme(theme);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, [theme]);

  const value = useMemo<ThemeCtx>(
    () => ({
      theme,
      update: (patch) => setTheme((t) => ({ ...t, ...patch })),
      reset: () => setTheme(DEFAULT_THEME),
    }),
    [theme],
  );

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useTheme() {
  const c = useContext(Ctx);
  if (!c) throw new Error("useTheme must be used inside ThemeProvider");
  return c;
}
