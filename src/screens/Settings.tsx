import { FolderSearch2, RotateCcw, Save } from "lucide-react";
import { useEffect, useState } from "react";
import {
  getList,
  getSetting,
  setSetting,
  getQuarantineDir,
  pickFolder,
  setList,
  setQuarantineDir,
  type QuarantineDir,
} from "../api";
import { LANGS, useI18n, type LangPref, type MsgKey } from "../i18n";
import { ALL_EXTS, FILE_TYPES } from "../review/types";
import { useTheme } from "../theme/ThemeProvider";
import { PRESETS, type ThemeMode } from "../theme/theme";
import { Screen } from "./Placeholder";

const MODES: { id: ThemeMode; label: MsgKey }[] = [
  { id: "system", label: "settings.appearance.system" },
  { id: "light", label: "settings.appearance.light" },
  { id: "dark", label: "settings.appearance.dark" },
];

const TYPES_KEY = "scan.file_types";
const EXCLUDED_KEY = "scan.excluded_paths";
const SIMILAR_KEY = "scan.similar";
const SENSITIVITY_KEY = "similar.sensitivity";
const SENSITIVITIES: { id: string; label: MsgKey }[] = [
  { id: "strict", label: "settings.scan.sensitivity.strict" },
  { id: "normal", label: "settings.scan.sensitivity.normal" },
  { id: "loose", label: "settings.scan.sensitivity.loose" },
];
const errMsg = (e: unknown) => (typeof e === "string" ? e : (e as Error)?.message ?? "Unknown error");

function Scanning() {
  const { t } = useI18n();
  const [exts, setExts] = useState<string[]>(ALL_EXTS);
  const [excluded, setExcluded] = useState<string[]>([]);
  const [similar, setSimilar] = useState(true);
  const [sensitivity, setSensitivity] = useState("normal");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getSetting(SIMILAR_KEY).then((v) => setSimilar(v !== "0")).catch((e) => setError(errMsg(e)));
    getSetting(SENSITIVITY_KEY)
      .then((v) => v && SENSITIVITIES.some((s) => s.id === v) && setSensitivity(v))
      .catch((e) => setError(errMsg(e)));
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

  const saveSetting = (key: string, value: string, apply: () => void) =>
    setSetting(key, value).then(
      () => {
        apply();
        setError(null);
      },
      (e) => setError(errMsg(e)),
    );

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
      <h2>{t("settings.scan.title")}</h2>
      {error && <div className="alert" role="alert">{error}</div>}
      <p className="muted">{t("settings.scan.applies")}</p>

      <div className="field">
        <span className="field-label">{t("settings.scan.types")}</span>
        <div className="chips" role="group" aria-label={t("settings.scan.types")}>
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
        <label className="inline check">
          <input
            type="checkbox"
            checked={similar}
            onChange={(e) => {
              const on = e.target.checked;
              void saveSetting(SIMILAR_KEY, on ? "1" : "0", () => setSimilar(on));
            }}
          />
          <span className="field-label">{t("settings.scan.similar")}</span>
        </label>
        <span className="muted">{t("settings.scan.similarHelp")}</span>
      </div>

      <div className="field">
        <span className="field-label" id="sens-label">{t("settings.scan.sensitivity")}</span>
        <div className="segmented" role="radiogroup" aria-labelledby="sens-label">
          {SENSITIVITIES.map((s) => (
            <button
              key={s.id}
              className={sensitivity === s.id ? "seg active" : "seg"}
              role="radio"
              aria-checked={sensitivity === s.id}
              onClick={() => void saveSetting(SENSITIVITY_KEY, s.id, () => setSensitivity(s.id))}
            >
              {t(s.label)}
            </button>
          ))}
        </div>
        <span className="muted">{t("settings.scan.sensitivityHelp")}</span>
      </div>

      <div className="field">
        <span className="field-label">{t("settings.scan.excluded")}</span>
        {excluded.length === 0 ? (
          <span className="muted">{t("settings.scan.noneExcluded")}</span>
        ) : (
          <ul className="excluded-list">
            {excluded.map((p) => (
              <li key={p}>
                <span className="folder-path" title={p}>{p}</span>
                <button
                  className="btn small"
                  onClick={() => void save(EXCLUDED_KEY, excluded.filter((x) => x !== p), setExcluded)}
                >
                  {t("common.remove")}
                </button>
              </li>
            ))}
          </ul>
        )}
        <button className="btn ghost" onClick={() => void addExcluded()}>{t("settings.scan.exclude")}</button>
      </div>
    </div>
  );
}

function QuarantineSettings() {
  const { t } = useI18n();
  const [dir, setDir] = useState<QuarantineDir | null>(null);
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    getQuarantineDir()
      .then((d) => {
        setDir(d);
        setValue(d.path);
      })
      .catch((e) => setError(errMsg(e)));
  }, []);

  async function apply(path: string) {
    setSaved(false);
    try {
      const d = await setQuarantineDir(path);
      setDir(d);
      setValue(d.path);
      setError(null);
      setSaved(true);
    } catch (e) {
      setError(errMsg(e));
    }
  }

  async function browse() {
    try {
      const p = await pickFolder();
      if (p) setValue(p);
    } catch (e) {
      setError(errMsg(e));
    }
  }

  const isDefault = !dir || dir.path === dir.default;
  const changed = !!dir && value.trim() !== dir.path;

  return (
    <div className="card form">
      <h2>{t("settings.q.title")}</h2>
      {error && <div className="alert" role="alert">{error}</div>}
      <label className="field">
        <span className="field-label">{t("settings.q.label")}</span>
        <div className="path-row">
          <input
            className="input grow mono"
            value={value}
            spellCheck={false}
            onChange={(e) => {
              setValue(e.target.value);
              setSaved(false);
            }}
            onKeyDown={(e) => e.key === "Enter" && changed && void apply(value)}
          />
          <button
            className="btn icon"
            title={t("settings.q.browse")}
            aria-label={t("settings.q.browse")}
            onClick={() => void browse()}
          >
            <FolderSearch2 size={18} aria-hidden="true" />
          </button>
          <button
            className="btn icon primary"
            title={t("settings.q.save")}
            aria-label={t("settings.q.save")}
            disabled={!changed}
            onClick={() => void apply(value)}
          >
            <Save size={18} aria-hidden="true" />
          </button>
          <button
            className="btn icon"
            title={t("settings.q.reset")}
            aria-label={t("settings.q.reset")}
            disabled={isDefault && !changed}
            onClick={() => void apply("")}
          >
            <RotateCcw size={18} aria-hidden="true" />
          </button>
        </div>
      </label>
      <p className="muted">
        {t("settings.q.note")}
        {saved && <strong> {t("settings.q.saved")}</strong>}
      </p>
    </div>
  );
}

function LanguageSettings() {
  const { t, lang, pref, setPref } = useI18n();
  const [error, setError] = useState<string | null>(null);

  const options: { id: LangPref; label: string }[] = [
    { id: "system", label: t("settings.lang.system") },
    ...LANGS.map((l) => ({ id: l as LangPref, label: t(`lang.${l}` as MsgKey) })),
  ];

  return (
    <div className="card form">
      <h2>{t("settings.lang.title")}</h2>
      {error && <div className="alert" role="alert">{error}</div>}
      <div className="field">
        <span className="field-label" id="lang-label">{t("settings.lang.label")}</span>
        <div className="segmented" role="radiogroup" aria-labelledby="lang-label">
          {options.map((o) => (
            <button
              key={o.id}
              role="radio"
              aria-checked={pref === o.id}
              className={pref === o.id ? "seg active" : "seg"}
              onClick={() => setPref(o.id).then(() => setError(null), (e) => setError(errMsg(e)))}
            >
              {o.label}
            </button>
          ))}
        </div>
      </div>
      <p className="muted">{t("settings.lang.current", { name: t(`lang.${lang}` as MsgKey) })}</p>
    </div>
  );
}

export default function Settings() {
  const { theme, update, reset } = useTheme();
  const { t } = useI18n();

  return (
    <Screen title={t("settings.title")} subtitle={t("settings.subtitle")}>
      <Scanning />
      <QuarantineSettings />
      <div className="card form">
        <h2>{t("settings.appearance.title")}</h2>

        <div className="field">
          <span className="field-label" id="mode-label">{t("settings.appearance.mode")}</span>
          <div className="segmented" role="radiogroup" aria-labelledby="mode-label">
            {MODES.map((m) => (
              <button
                key={m.id}
                role="radio"
                aria-checked={theme.mode === m.id}
                className={theme.mode === m.id ? "seg active" : "seg"}
                onClick={() => update({ mode: m.id })}
              >
                {t(m.label)}
              </button>
            ))}
          </div>
        </div>

        <div className="field">
          <span className="field-label">{t("settings.appearance.presets")}</span>
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
            <span className="field-label">{t("settings.appearance.primary")}</span>
            <input
              type="color"
              value={theme.primary}
              onChange={(e) => update({ primary: e.target.value })}
            />
          </label>
          <label className="color">
            <span className="field-label">{t("settings.appearance.secondary")}</span>
            <input
              type="color"
              value={theme.secondary}
              onChange={(e) => update({ secondary: e.target.value })}
            />
          </label>
        </div>

        <label className="field">
          <span className="field-label">{t("settings.appearance.glow", { value: theme.glow })}</span>
          <input
            type="range"
            min={0}
            max={100}
            value={theme.glow}
            onChange={(e) => update({ glow: Number(e.target.value) })}
          />
        </label>

        <label className="field">
          <span className="field-label">{t("settings.appearance.radius", { value: theme.radius })}</span>
          <input
            type="range"
            min={0}
            max={24}
            value={theme.radius}
            onChange={(e) => update({ radius: Number(e.target.value) })}
          />
        </label>

        <div className="field">
          <button className="btn ghost" onClick={reset}>{t("settings.appearance.reset")}</button>
        </div>
      </div>

      <LanguageSettings />
    </Screen>
  );
}
