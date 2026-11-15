import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { getSetting, setSetting } from "../api";
import { setLocale } from "../format";
import { en, type Key } from "./en";
import { it } from "./it";

export type Lang = "en" | "it";
export type LangPref = "system" | Lang;
export const LANGS: Lang[] = ["en", "it"];

const SETTING_KEY = "language";
const DICTS: Record<Lang, Record<Key, string>> = { en, it };

/** A key as callers write it: plural families are referred to without their suffix. */
type Base<K extends string> = K extends `${infer B}_${"one" | "other"}` ? B : K;
export type MsgKey = Base<Key>;
type Vars = Record<string, string | number>;

export function resolveLang(pref: LangPref): Lang {
  if (pref !== "system") return pref;
  const nav = typeof navigator === "undefined" ? "en" : navigator.language;
  return nav.toLowerCase().startsWith("it") ? "it" : "en";
}

export async function loadLangPref(): Promise<LangPref> {
  try {
    const v = await getSetting(SETTING_KEY);
    if (v === "system" || v === "en" || v === "it") return v;
  } catch (e) {
    console.error("Failed to load language", e);
  }
  return "system";
}

function translate(lang: Lang, key: MsgKey, vars?: Vars): string {
  const d = DICTS[lang] as Record<string, string>;
  let msg: string | undefined;
  if (typeof vars?.n === "number") {
    const rule = new Intl.PluralRules(lang).select(vars.n);
    msg = d[`${key}_${rule}`] ?? d[`${key}_other`];
  }
  msg ??= d[key] ?? (en as Record<string, string>)[key] ?? key;
  return vars ? msg.replace(/\{(\w+)\}/g, (m, name) => String(vars[name] ?? m)) : msg;
}

interface I18nCtx {
  t: (key: MsgKey, vars?: Vars) => string;
  lang: Lang;
  pref: LangPref;
  setPref: (p: LangPref) => Promise<void>;
}

const Ctx = createContext<I18nCtx | null>(null);

export function LanguageProvider({ initial, children }: { initial: LangPref; children: ReactNode }) {
  const [pref, setPrefState] = useState<LangPref>(initial);
  const lang = resolveLang(pref);
  // Set synchronously so number/date formatting is right on the very first render of children.
  setLocale(lang);

  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);

  const setPref = useCallback(async (p: LangPref) => {
    setPrefState(p);
    await setSetting(SETTING_KEY, p);
  }, []);

  const t = useCallback((key: MsgKey, vars?: Vars) => translate(lang, key, vars), [lang]);
  const value = useMemo(() => ({ t, lang, pref, setPref }), [t, lang, pref, setPref]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useI18n(): I18nCtx {
  const c = useContext(Ctx);
  if (!c) throw new Error("useI18n must be used inside <LanguageProvider>");
  return c;
}
