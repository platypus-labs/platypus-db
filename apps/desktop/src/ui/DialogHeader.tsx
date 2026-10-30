/**
 * The head of a dialog: its title, and a Close button on the right. The same in every dialog
 * built on `.modal`, so the way out is always where the user looks for it.
 */
import type { ReactNode } from "react";
import { t } from "../i18n";
import { Icon } from "./Icon";

export function DialogHeader({ title, onClose, disabled }: { title: ReactNode; onClose: () => void; disabled?: boolean }) {
  return (
    <div className="modal-head">
      <h2>{title}</h2>
      <button type="button" className="icon modal-close" title={t("Close")} aria-label={t("Close")} disabled={disabled} onClick={onClose}>
        <Icon name="close" size={14} />
      </button>
    </div>
  );
}
