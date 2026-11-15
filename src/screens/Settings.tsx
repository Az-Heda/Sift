import { useEffect, useState } from "react";
import { getList, pickFolder, setList } from "../api";
import { ALL_EXTS, FILE_TYPES } from "../review/types";
import { useTheme } from "../theme/ThemeProvider";
import { PRESETS, type ThemeMode } from "../theme/theme";
import { Screen } from "./Placeholder";

const MODES: { id: ThemeMode; label: string }[] = [
  { id: "system", label: "System" },
  { id: "light", label: "Light" },
  { id: "dark", label: "Dark" },
];

const TYPES_KEY = "scan.file_types";
const EXCLUDED_KEY = "scan.excluded_paths";
const errMsg = (e: unknown) => (typeof e === "string" ? e : (e as Error)?.message ?? "Unknown error");

function Scanning() {
  const [exts, setExts] = useState<string[]>(ALL_EXTS);
  const [excluded, setExcluded] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getList(TYPES_KEY).then((l) => l && setExts(l)).catch((e) => setError(errMsg(e)));
    getList(EXCLUDED_KEY).then((l) => l && setExcluded(l)).catch((e) => setError(errMsg(e)));
  }, []);

  const save = (key: string, list: string[], apply: (l: string[]) => void) =>
    setList(key, list).then(
      () => {
        apply(list);
        setError(null);
      },
      (e) => setError(errMsg(e)),
    );

  const typeOn = (t: (typeof FILE_TYPES)[number]) => t.exts.some((e) => exts.includes(e));
  const toggleType = (t: (typeof FILE_TYPES)[number]) => {
    const next = typeOn(t)
      ? exts.filter((e) => !(t.exts as readonly string[]).includes(e))
      : [...exts, ...t.exts];
    return save(TYPES_KEY, next, setExts);
  };

  async function addExcluded() {
    try {
      const p = await pickFolder();
      if (p && !excluded.includes(p)) await save(EXCLUDED_KEY, [...excluded, p], setExcluded);
    } catch (e) {
      setError(errMsg(e));
    }
  }

  return (
    <div className="card form">
      <h2>Scanning</h2>
      {error && <div className="alert" role="alert">{error}</div>}
      <p className="muted">Changes apply to the next scan.</p>

      <div className="field">
        <span className="field-label">File types to scan</span>
        <div className="chips" role="group" aria-label="File types to scan">
          {FILE_TYPES.map((t) => (
            <button
              key={t.id}
              className={typeOn(t) ? "chip active" : "chip"}
              aria-pressed={typeOn(t)}
              onClick={() => void toggleType(t)}
            >
              {t.label}
            </button>
          ))}
        </div>
      </div>

      <div className="field">
        <span className="field-label">Excluded folders</span>
        {excluded.length === 0 ? (
          <span className="muted">
            None. System folders such as .git, node_modules and caches are always skipped.
          </span>
        ) : (
          <ul className="excluded-list">
            {excluded.map((p) => (
              <li key={p}>
                <span className="folder-path" title={p}>{p}</span>
                <button
                  className="btn small"
                  onClick={() => void save(EXCLUDED_KEY, excluded.filter((x) => x !== p), setExcluded)}
                >
                  Remove
                </button>
              </li>
            ))}
          </ul>
        )}
        <button className="btn ghost" onClick={() => void addExcluded()}>Exclude a folder…</button>
      </div>
    </div>
  );
}

export default function Settings() {
  const { theme, update, reset } = useTheme();

  return (
    <Screen title="Settings" subtitle="Appearance, excluded folders and scanning preferences.">
      <Scanning />
      <div className="card form">
        <h2>Appearance</h2>

        <div className="field">
          <span className="field-label" id="mode-label">Mode</span>
          <div className="segmented" role="radiogroup" aria-labelledby="mode-label">
            {MODES.map((m) => (
              <button
                key={m.id}
                role="radio"
                aria-checked={theme.mode === m.id}
                className={theme.mode === m.id ? "seg active" : "seg"}
                onClick={() => update({ mode: m.id })}
              >
                {m.label}
              </button>
            ))}
          </div>
        </div>

        <div className="field">
          <span className="field-label">Presets</span>
          <div className="presets">
            {PRESETS.map((p) => (
              <button
                key={p.name}
                className="preset"
                onClick={() => update({ primary: p.primary, secondary: p.secondary })}
              >
                <span
                  className="swatch"
                  style={{ background: `linear-gradient(135deg, ${p.primary}, ${p.secondary})` }}
                />
                {p.name}
              </button>
            ))}
          </div>
        </div>

        <div className="field row">
          <label className="color">
            <span className="field-label">Primary</span>
            <input
              type="color"
              value={theme.primary}
              onChange={(e) => update({ primary: e.target.value })}
            />
          </label>
          <label className="color">
            <span className="field-label">Secondary</span>
            <input
              type="color"
              value={theme.secondary}
              onChange={(e) => update({ secondary: e.target.value })}
            />
          </label>
        </div>

        <label className="field">
          <span className="field-label">Glow ({theme.glow}%)</span>
          <input
            type="range"
            min={0}
            max={100}
            value={theme.glow}
            onChange={(e) => update({ glow: Number(e.target.value) })}
          />
        </label>

        <label className="field">
          <span className="field-label">Corner radius ({theme.radius}px)</span>
          <input
            type="range"
            min={0}
            max={24}
            value={theme.radius}
            onChange={(e) => update({ radius: Number(e.target.value) })}
          />
        </label>

        <div className="field">
          <button className="btn ghost" onClick={reset}>Reset to defaults</button>
        </div>
      </div>
    </Screen>
  );
}
