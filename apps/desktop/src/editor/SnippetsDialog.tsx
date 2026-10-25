import { useState } from "react";
import { toCommandError } from "../api";
import { DEFAULT_SNIPPETS, type Snippet, saveSnippets, snippets } from "./snippets";
import { t, tNodes } from "../i18n";

/** Edits the snippet list: a name typed then Tab writes the template. */
export function SnippetsDialog({ onClose }: { onClose: () => void }) {
  const [list, setList] = useState<Snippet[]>(snippets);
  const [error, setError] = useState<string | null>(null);
  const patch = (i: number, p: Partial<Snippet>) => setList((l) => l.map((s, j) => (j === i ? { ...s, ...p } : s)));

  async function save() {
    const names = list.map((s) => s.name.trim());
    if (names.some((n) => !/^[A-Za-z_]\w*$/.test(n))) return setError(t("Names are letters, digits and _, starting with a letter."));
    if (new Set(names).size !== names.length) return setError(t("Two snippets have the same name."));
    try {
      await saveSnippets(list.map((s) => ({ ...s, name: s.name.trim() })));
      onClose();
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <form
        className="modal snippets-dialog"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <h2>{t("Snippets")}</h2>
        <p className="muted small">
          {tNodes("Type a name in the editor and press Tab. {field} marks a field to fill; Tab goes to the next one.", {
            field: <code>#{"{field}"}</code>,
          })}
        </p>
        <div className="snippet-list">
          {list.map((s, i) => (
            <div className="snippet-row" key={i}>
              <input aria-label={t("Name")} value={s.name} onChange={(e) => patch(i, { name: e.target.value })} />
              <textarea
                aria-label={t("Template")}
                rows={Math.min(6, s.template.split("\n").length)}
                value={s.template}
                onChange={(e) => patch(i, { template: e.target.value })}
              />
              <button type="button" className="icon" title={t("Delete")} aria-label={t("Delete")} onClick={() => setList((l) => l.filter((_, j) => j !== i))}>
                ×
              </button>
            </div>
          ))}
        </div>
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          <button type="button" onClick={() => setList((l) => [...l, { name: "", template: "" }])}>
            {t("Add")}
          </button>
          <button type="button" className="link" onClick={() => setList(DEFAULT_SNIPPETS)}>
            {t("Restore defaults")}
          </button>
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="submit" className="primary">
            {t("Save")}
          </button>
        </div>
      </form>
    </div>
  );
}
