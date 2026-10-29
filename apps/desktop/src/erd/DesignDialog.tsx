import { useState } from "react";
import type { DriverKind, TableDesign } from "../api";
import { DesignForm, type TableEditorSection } from "../table-editor/DesignForm";
import { t } from "../i18n";
import { type DesignProblem, designProblem } from "./edits";

/** What the dialog is for: a table to create, a table created before and edited again, or a
 *  table of the database. */
export type DesignKind = "add" | "new" | "existing";

interface Props {
  driver: DriverKind;
  schema: string;
  design: TableDesign;
  kind: DesignKind;
  /** Whether the design differs from the database's table: only then it can be reverted. */
  changed: boolean;
  /** The names of the other tables of the schema, the new ones included: this table cannot take one. */
  taken: string[];
  /** The columns of the tables of the diagram, for the hints. */
  columns: Record<string, string[]>;
  onSave: (design: TableDesign) => void;
  /** Marks a table of the database as dropped. */
  onDrop: () => void;
  /** Removes a table created or reverts the changes of one of the database. */
  onRevert: () => void;
  onClose: () => void;
}

const PROBLEMS: Record<DesignProblem, () => string> = {
  name: () => t("The table needs a name."),
  duplicate: () => t("The schema has a table with this name."),
  columns: () => t("The table needs at least one column."),
  column_name: () => t("Every column needs a name."),
  column_type: () => t("Every column needs a type."),
  duplicate_column: () => t("Two columns have the same name."),
};

/** The form of a table of the diagram in edit mode: a blank table to add, or the design of
 *  one to change. The name of a table of the database is kept: a rename is a new table. */
export function DesignDialog({ driver, schema, design: initial, kind, changed, taken, columns, onSave, onDrop, onRevert, onClose }: Props) {
  const [design, setDesign] = useState(initial);
  const [section, setSection] = useState<TableEditorSection>("columns");
  const [error, setError] = useState<string | null>(null);

  function save() {
    const problem = designProblem(design, taken);
    if (problem) return setError(PROBLEMS[problem]());
    onSave(design);
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal wide design-dialog" role="dialog" aria-label={t("Table design")} onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{kind === "add" ? t("Add table") : t("Table {name}", { name: initial.name })}</h2>
        <label className="table-name">
          <input
            value={design.name}
            placeholder={t("Table name")}
            aria-label={t("Table name")}
            autoFocus={kind === "add"}
            readOnly={kind === "existing"}
            onChange={(e) => setDesign((d) => ({ ...d, name: e.target.value }))}
          />
          <span className="muted small">{t("in {schema}", { schema })}</span>
        </label>
        <DesignForm
          driver={driver}
          design={design}
          onChange={(change) => setDesign(change)}
          schema={schema}
          catalog={{ [schema]: columns }}
          section={section}
          onSection={setSection}
          notice={error && <div className="notice error small">{error}</div>}
          idPrefix="erd-design"
        />
        <div className="actions">
          {kind === "existing" && (
            <button type="button" className="danger" onClick={onDrop}>
              {t("Drop table")}
            </button>
          )}
          {kind === "new" && (
            <button type="button" className="danger" onClick={onRevert}>
              {t("Remove")}
            </button>
          )}
          {kind === "existing" && changed && (
            <button type="button" onClick={onRevert}>
              {t("Revert")}
            </button>
          )}
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="button" className="primary" onClick={save}>
            {t("Save")}
          </button>
        </div>
      </div>
    </div>
  );
}
