import type { ReactNode } from "react";
import { useI18n, type MsgKey } from "../i18n";

export type ScreenId =
  | "dashboard"
  | "folders"
  | "duplicates"
  | "compare"
  | "storage"
  | "quarantine"
  | "settings";

const svg = (d: ReactNode) => (
  <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor"
    strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    {d}
  </svg>
);

export const NAV: { id: ScreenId; label: MsgKey; icon: ReactNode }[] = [
  { id: "dashboard", label: "nav.dashboard", icon: svg(<><rect x="3" y="3" width="7" height="9" /><rect x="14" y="3" width="7" height="5" /><rect x="14" y="12" width="7" height="9" /><rect x="3" y="16" width="7" height="5" /></>) },
  { id: "folders", label: "nav.folders", icon: svg(<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />) },
  { id: "duplicates", label: "nav.duplicates", icon: svg(<><rect x="8" y="8" width="13" height="13" rx="2" /><path d="M16 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3" /></>) },
  { id: "compare", label: "nav.compare", icon: svg(<><path d="M12 3v18" /><rect x="3" y="5" width="6" height="14" rx="1" /><rect x="15" y="5" width="6" height="14" rx="1" /></>) },
  { id: "storage", label: "nav.storage", icon: svg(<><ellipse cx="12" cy="5" rx="8" ry="3" /><path d="M4 5v6c0 1.7 3.6 3 8 3s8-1.3 8-3V5" /><path d="M4 11v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6" /></>) },
  { id: "quarantine", label: "nav.quarantine", icon: svg(<><rect x="3" y="4" width="18" height="5" rx="1" /><path d="M5 9v9a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V9" /><path d="M10 13h4" /></>) },
  { id: "settings", label: "nav.settings", icon: svg(<><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" /></>) },
];

export default function Sidebar({
  active,
  onSelect,
}: {
  active: ScreenId;
  onSelect: (id: ScreenId) => void;
}) {
  const { t } = useI18n();
  return (
    <aside className="sidebar">
      <div className="brand">
        <span className="brand-mark" aria-hidden="true" />
        <div>
          <div className="brand-name">Sift</div>
          <div className="brand-tag">{t("app.tagline")}</div>
        </div>
      </div>
      <nav aria-label={t("nav.main")}>
        {NAV.map((n) => (
          <button
            key={n.id}
            className={n.id === active ? "nav-item active" : "nav-item"}
            aria-current={n.id === active ? "page" : undefined}
            onClick={() => onSelect(n.id)}
          >
            {n.icon}
            <span>{t(n.label)}</span>
          </button>
        ))}
      </nav>
    </aside>
  );
}
