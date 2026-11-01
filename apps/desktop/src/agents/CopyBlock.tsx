import { t } from "../i18n";
import { copyText } from "../ui/copy";

/** A command to paste somewhere else, with its Copy button. */
export function CopyBlock({ label, text, field }: { label: string; text: string; field: string }) {
  return (
    <div className="agent-command">
      <div className="row tight">
        <strong className="small">{label}</strong>
        <span className="spacer" />
        <button type="button" onClick={() => void copyText(text)} data-copy={field}>
          {t("Copy")}
        </button>
      </div>
      <pre className="sql-preview" data-field={field}>
        {text}
      </pre>
    </div>
  );
}
