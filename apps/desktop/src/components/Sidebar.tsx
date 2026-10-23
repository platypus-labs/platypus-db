import type { SavedConnection } from "../api";

interface Props {
  connections: SavedConnection[];
  error: string | null;
  onNew: () => void;
  onEdit: (c: SavedConnection) => void;
  onDelete: (c: SavedConnection) => void;
  onOpen: (c: SavedConnection) => void;
}

export function Sidebar({ connections, error, onNew, onEdit, onDelete, onOpen }: Props) {
  return (
    <aside className="sidebar">
      <div className="sidebar-header">
        <span className="sidebar-title">Connections</span>
        <button className="icon" title="New connection" onClick={onNew}>
          +
        </button>
      </div>
      {error && <div className="notice error small">{error}</div>}
      {connections.length === 0 && !error && (
        <button className="empty-cta" onClick={onNew}>
          Add your first connection
        </button>
      )}
      <ul className="connection-list">
        {connections.map((c) => (
          <li
            key={c.id}
            className="connection"
            title={`${c.user}@${c.host}:${c.port ?? ""}/${c.database}`}
            onDoubleClick={() => onOpen(c)}
          >
            <span className="dot" style={{ background: c.color ?? "var(--border)" }} />
            <span className="connection-name">{c.name}</span>
            <span className="connection-actions">
              <button className="icon" title="Open SQL editor" onClick={() => onOpen(c)}>
                ▶
              </button>
              <button className="icon" title="Edit" onClick={() => onEdit(c)}>
                ✎
              </button>
              <button className="icon" title="Delete" onClick={() => onDelete(c)}>
                ×
              </button>
            </span>
          </li>
        ))}
      </ul>
    </aside>
  );
}
