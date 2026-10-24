interface Props {
  statements: string[];
  busy: boolean;
  /** Saving changes a production database: said in the dialog. */
  production?: boolean;
  onSave: () => void;
  onClose: () => void;
}

/** The statements that saving would run, before they run. */
export function PreviewDialog({ statements, busy, production = false, onSave, onClose }: Props) {
  const script = statements.map((s) => `${s};`).join("\n");
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal wide" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>Changes to save</h2>
        {production && <div className="notice warn">This is a production connection: check the SQL before saving.</div>}
        <p className="muted small">
          {statements.length} {statements.length === 1 ? "statement runs" : "statements run"} in one transaction:
          all of them, or none if one fails.
        </p>
        <pre className="sql-preview">{script}</pre>
        <div className="actions">
          <button type="button" onClick={() => void navigator.clipboard.writeText(script)}>
            Copy
          </button>
          <span className="spacer" />
          <button type="button" onClick={onClose} autoFocus>
            Close
          </button>
          <button type="button" className="primary" onClick={onSave} disabled={busy}>
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
