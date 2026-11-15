import { useTheme } from "../theme/ThemeProvider";
import { PRESETS, type ThemeMode } from "../theme/theme";
import { Screen } from "./Placeholder";

const MODES: { id: ThemeMode; label: string }[] = [
  { id: "system", label: "System" },
  { id: "light", label: "Light" },
  { id: "dark", label: "Dark" },
];

export default function Settings() {
  const { theme, update, reset } = useTheme();

  return (
    <Screen title="Settings" subtitle="Appearance, excluded folders and scanning preferences.">
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
