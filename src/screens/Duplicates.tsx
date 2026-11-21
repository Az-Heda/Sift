import { useEffect, useMemo, useState } from "react";
import {
  deleteFiles,
  listFolders,
  listGroups,
  openContainingFolder,
  quarantineFiles,
  revealFile,
  type CleanupResult,
  type DupGroup,
  type DupMode,
  type Folder,
} from "../api";
import ConfirmDialog from "../components/ConfirmDialog";
import CopyCard from "../components/CopyCard";
import FilterBar from "../components/FilterBar";
import { formatBytes } from "../format";
import { useI18n, type MsgKey } from "../i18n";
import { applyFilters, isFiltered } from "../review/filters";
import { applyRule, RULES, type KeepRule } from "../review/rules";
import { useReview } from "../review/ReviewProvider";
import { useScan } from "../scan/ScanProvider";
import { Empty, Screen } from "./Placeholder";

const PAGE = 25;
const MODES: { id: DupMode; label: MsgKey }[] = [
  { id: "exact", label: "dup.mode.exact" },
  { id: "similar", label: "dup.mode.similar" },
];
const errMsg = (e: unknown) => (typeof e === "string" ? e : (e as Error)?.message ?? "Unknown error");

export default function Duplicates() {
  const { version, invalidate, running } = useScan();
  const { t } = useI18n();
  const review = useReview();
  const { selected, filters, mode } = review;
  const [groups, setGroups] = useState<DupGroup[] | null>(null);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [shown, setShown] = useState(PAGE);
  const [rule, setRule] = useState<KeepRule>("recommended");
  const [master, setMaster] = useState("");
  const [ruleNote, setRuleNote] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<"quarantine" | "delete" | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [failures, setFailures] = useState<CleanupResult["failed"]>([]);

  useEffect(() => {
    let current = true;
    setGroups(null);
    listGroups(mode)
      .then((g) => {
        if (!current) return;
        setGroups(g);
        setError(null);
      })
      .catch((e) => current && setError(errMsg(e)));
    listFolders().then(setFolders).catch(() => setFolders([]));
    return () => {
      current = false;
    };
  }, [version, mode]);

  const visible = useMemo(() => (groups ? applyFilters(groups, filters) : []), [groups, filters]);

  // Restart paging whenever the filtered list changes.
  useEffect(() => setShown(PAGE), [visible]);

  const sizeById = useMemo(() => {
    const m = new Map<number, number>();
    groups?.forEach((g) => g.files.forEach((f) => m.set(f.id, f.size)));
    return m;
  }, [groups]);
  const selectedBytes = useMemo(
    () => [...selected].reduce((n, id) => n + (sizeById.get(id) ?? 0), 0),
    [selected, sizeById],
  );

  const act = (fn: (id: number) => Promise<void>) => (id: number) =>
    fn(id).then(() => setError(null), (e) => setError(errMsg(e)));

  // Groups where every copy is selected: cleanup is blocked until one is deselected.
  const emptied = useMemo(
    () => (groups ?? []).filter((g) => g.files.every((f) => selected.has(f.id))).length,
    [groups, selected],
  );

  function selectByRule() {
    const { ids, skipped } = applyRule(visible, rule, master);
    review.replace(ids);
    setRuleNote(skipped > 0 ? t("dup.skipped", { n: skipped }) : null);
  }

  async function runCleanup(kind: "quarantine" | "delete") {
    setConfirm(null);
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const r = await (kind === "quarantine" ? quarantineFiles : deleteFiles)([...selected], mode);
      setFailures(r.failed);
      setNotice(t(kind === "quarantine" ? "dup.moved" : "dup.deleted", { n: r.done, size: formatBytes(r.bytes) }));
      invalidate(); // refetches the list and clears the selection
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setBusy(false);
    }
  }

  const cleanupBlocked = busy || running || selected.size === 0 || emptied > 0;

  return (
    <Screen
      title={t("dup.title")}
      subtitle={t(mode === "similar" ? "dup.subtitleSimilar" : "dup.subtitle")}
    >
      <div className="segmented" role="radiogroup" aria-label={t("dup.mode.aria")}>
        {MODES.map((m) => (
          <button
            key={m.id}
            className={mode === m.id ? "seg active" : "seg"}
            role="radio"
            aria-checked={mode === m.id}
            onClick={() => review.setMode(m.id)}
          >
            {t(m.label)}
          </button>
        ))}
      </div>
      {mode === "similar" && <p className="muted">{t("dup.similarNote")}</p>}
      {error && <div className="alert" role="alert">{error}</div>}
      {notice && <div className="notice" role="status">{notice}</div>}
      {failures.length > 0 && (
        <details className="warn">
          <summary>{t("common.failed", { n: failures.length })}</summary>
          <ul className="failures">
            {failures.map((f, i) => (
              <li key={i}><code>{f.path}</code>: {f.reason}</li>
            ))}
          </ul>
        </details>
      )}
      {groups === null ? null : groups.length === 0 ? (
        <Empty text={t(mode === "similar" ? "dup.emptySimilar" : "dup.empty")} />
      ) : (
        <>
          <FilterBar
            filters={filters}
            folders={folders}
            onChange={review.setFilters}
            onReset={review.resetFilters}
          />
          <div className="toolbar">
            <label className="inline">
              <span className="field-label">{t("dup.keep")}</span>
              <select className="input" value={rule} onChange={(e) => setRule(e.target.value as KeepRule)}>
                {RULES.map((r) => (
                  <option key={r.id} value={r.id}>{t(r.label)}</option>
                ))}
              </select>
            </label>
            {rule === "master" && (
              <select
                className="input"
                aria-label={t("dup.masterAria")}
                value={master}
                onChange={(e) => setMaster(e.target.value)}
              >
                <option value="">{t("dup.chooseMaster")}</option>
                {folders.map((f) => (
                  <option key={f.id} value={f.path}>{f.path}</option>
                ))}
              </select>
            )}
            <button
              className="btn"
              onClick={selectByRule}
              disabled={visible.length === 0 || (rule === "master" && !master)}
            >
              {t("dup.selectOthers")}
            </button>
            <button className="btn" onClick={review.clear} disabled={selected.size === 0}>
              {t("common.clearSelection")}
            </button>
            <span className="muted" aria-live="polite">
              {selected.size > 0
                ? t("common.selectedInfo", { n: selected.size, size: formatBytes(selectedBytes) })
                : t("dup.groupsOf", { shown: visible.length, total: groups.length })}
            </span>
          </div>
          {ruleNote && <div className="muted">{ruleNote}</div>}

          {visible.length === 0 ? (
            <Empty text={isFiltered(filters) ? t("dup.noMatch") : t("dup.nothing")} />
          ) : (
            <ul className="group-list">
              {visible.slice(0, shown).map((g) => {
                const allSelected = g.files.every((f) => selected.has(f.id));
                return (
                  <li className="card group" key={g.sha256}>
                    <div className="group-head">
                      <strong>
                        {t(g.kind === "similar" ? "count.similar" : "count.copies", { n: g.files.length })}
                      </strong>
                      <span className="muted">
                        {g.kind === "similar"
                          ? t("dup.recoverableSimilar", { wasted: formatBytes(g.wasted) })
                          : t("dup.recoverable", { size: formatBytes(g.size), wasted: formatBytes(g.wasted) })}
                      </span>
                    </div>
                    {allSelected && (
                      <div className="warn" role="status">
                        {t("dup.allSelected")}
                      </div>
                    )}
                    <div className="copies">
                      {g.files.map((f) => (
                        <CopyCard
                          key={f.id}
                          file={f}
                          recommended={f.id === g.keep_id}
                          selected={selected.has(f.id)}
                          onToggle={() => review.toggle(f.id)}
                          onReveal={() => act(revealFile)(f.id)}
                          onOpenFolder={() => act(openContainingFolder)(f.id)}
                        />
                      ))}
                    </div>
                    <div className="group-actions">
                      <button className="btn small" onClick={() => review.keepOnly(g, g.keep_id)}>
                        {t("dup.keepRecommended")}
                      </button>
                      <button className="btn small primary" onClick={() => review.openCompare(g)}>
                        {t("dup.compare")}
                      </button>
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
          {shown < visible.length && (
            <div className="toolbar">
              <button className="btn" onClick={() => setShown((n) => n + PAGE)}>
                {t("dup.showMore", { n: visible.length - shown })}
              </button>
            </div>
          )}

          {selected.size > 0 && (
            <div className="card actionbar" role="region" aria-label={t("dup.cleanupRegion")}>
              <span>
                {emptied > 0 ? t("dup.blocked", { n: emptied }) : t("dup.nothingChanged")}
              </span>
              <div className="actionbar-buttons">
                <button className="btn primary" disabled={cleanupBlocked} onClick={() => setConfirm("quarantine")}>
                  {t("dup.moveToQuarantine")}
                </button>
                <button className="btn danger" disabled={cleanupBlocked} onClick={() => setConfirm("delete")}>
                  {t("dup.deletePermanently")}
                </button>
              </div>
            </div>
          )}
        </>
      )}
      <ConfirmDialog
        open={confirm === "quarantine"}
        title={t("dup.q.title")}
        tone="primary"
        confirmLabel={t("dup.q.confirm", { n: selected.size })}
        onCancel={() => setConfirm(null)}
        onConfirm={() => void runCleanup("quarantine")}
      >
        <p>{t("dup.q.body", { n: selected.size, size: formatBytes(selectedBytes) })}</p>
        {mode === "similar" && <p><strong>{t("dup.similarWarn")}</strong></p>}
        <p>{t("dup.q.hint")}</p>
      </ConfirmDialog>
      <ConfirmDialog
        open={confirm === "delete"}
        title={t("dup.d.title")}
        confirmLabel={t("dup.d.confirm", { n: selected.size })}
        onCancel={() => setConfirm(null)}
        onConfirm={() => void runCleanup("delete")}
      >
        <p>{t("dup.d.body", { n: selected.size, size: formatBytes(selectedBytes) })}</p>
        {mode === "similar" && <p><strong>{t("dup.similarWarn")}</strong></p>}
        <p><strong>{t("dup.d.warn")}</strong> {t("dup.d.hint")}</p>
      </ConfirmDialog>
    </Screen>
  );
}
