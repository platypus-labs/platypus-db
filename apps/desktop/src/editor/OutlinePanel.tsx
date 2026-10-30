import { useEffect, useState } from "react";
import { api, type DriverKind, type StatementSpan } from "../api";
import { t } from "../i18n";
import { EmptyState } from "../ui/State";
import { Icon } from "../ui/Icon";

interface Props {
  text: string;
  driver: DriverKind;
  onSelect: (from: number, to: number) => void;
  onClose: () => void;
}

/** First line of a statement, for the list. */
function headline(text: string) {
  const line = text.split("\n").find((l) => l.trim() && !l.trim().startsWith("--")) ?? text;
  return line.trim().slice(0, 80);
}

/** The editor's statements in order; a click selects one. */
export function OutlinePanel({ text, driver, onSelect, onClose }: Props) {
  const [statements, setStatements] = useState<StatementSpan[]>([]);
  useEffect(() => {
    let live = true;
    const t = setTimeout(() => {
      api.analyzeSql(text, driver, 0).then(
        (a) => live && setStatements(a.statements),
        () => {},
      );
    }, 300);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [text, driver]);
  return (
    <aside className="history side-panel outline-panel">
      <div className="history-header">
        <strong>{t("Outline")}</strong>
        <span className="spacer" />
        <button className="icon" title={t("Close")} aria-label={t("Close")} onClick={onClose}>
          <Icon name="close" />
        </button>
      </div>
      <ul className="history-list">
        {statements.map((s, i) => (
          <li key={`${s.start}`} className="outline-row" onClick={() => onSelect(s.start, s.end)} title={t("Select the statement")}>
            <span className={`outline-kind ${s.kind}`}>{s.kind === "command" ? "@" : s.kind}</span>
            <span className="outline-text">
              {i + 1}. {headline(text.slice(s.start, s.end))}
            </span>
          </li>
        ))}
        {statements.length === 0 && <li className="history-empty"><EmptyState compact title={t("No statements")} /></li>}
      </ul>
    </aside>
  );
}
