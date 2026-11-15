import { useI18n } from "../i18n";
import { useEffect, useRef, type ReactNode } from "react";

export default function ConfirmDialog({
  open,
  title,
  children,
  confirmLabel,
  tone = "danger",
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  children: ReactNode;
  confirmLabel: string;
  /** "danger" for destructive actions, "primary" for reversible ones. */
  tone?: "danger" | "primary";
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { t } = useI18n();
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (open && !d.open) d.showModal();
    if (!open && d.open) d.close();
  }, [open]);

  return (
    <dialog
      ref={ref}
      className="dialog"
      aria-labelledby="confirm-title"
      onCancel={(e) => {
        e.preventDefault(); // let React state drive closing
        onCancel();
      }}
      onClick={(e) => {
        if (e.target === ref.current) onCancel(); // backdrop click
      }}
    >
      <h2 id="confirm-title">{title}</h2>
      <div className="dialog-body">{children}</div>
      <div className="dialog-actions">
        <button className="btn" autoFocus onClick={onCancel}>
          {t("common.cancel")}
        </button>
        <button className={`btn ${tone}`} onClick={onConfirm}>
          {confirmLabel}
        </button>
      </div>
    </dialog>
  );
}
