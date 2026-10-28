import { useState } from "react";
import { t } from "../i18n";

/** A command to paste somewhere else, with its Copy button. */
export function CopyBlock({ label, text, field }: { label: string; text: string; field: string }) {
  const [copied, setCopied] = useState(false);
  const copy = () =>
    navigator.clipboard.writeText(text).then(
      () => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      },
      () => setCopied(false),
    );
  return (
    <div className="agent-command">
      <div className="row tight">
        <strong className="small">{label}</strong>
        <span className="spacer" />
        <button type="button" onClick={() => void copy()} data-copy={field}>
          {t(copied ? "Copied" : "Copy")}
        </button>
      </div>
      <pre className="sql-preview" data-field={field}>
        {text}
      </pre>
    </div>
  );
}
