import { useEffect, useState } from "react";
import { appInfo, type AppInfo } from "./api";

export function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    appInfo().then(setInfo, (e: unknown) => setError(String(e)));
  }, []);

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="sidebar-title">Connections</div>
        <div className="empty">No connections yet</div>
      </aside>
      <main className="workspace">
        <h1>PlatypusDB</h1>
        {info && (
          <p className="muted">
            v{info.version} · {info.os}/{info.arch}
          </p>
        )}
        {error && <p className="error">Backend unreachable: {error}</p>}
      </main>
    </div>
  );
}
