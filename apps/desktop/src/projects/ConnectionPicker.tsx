import { useState } from "react";
import type { SavedConnection } from "../api";
import { DriverIcon } from "../components/DriverPicker";
import { toggled } from "../sets";
import { t } from "../i18n";

interface Props {
  connections: SavedConnection[];
  picked: Set<string>;
  onChange: (picked: Set<string>) => void;
}

/** A searchable list of connections with a checkbox and the driver's logo each. */
export function ConnectionPicker({ connections, picked, onChange }: Props) {
  const [filter, setFilter] = useState("");
  const needle = filter.trim().toLowerCase();
  const shown = connections.filter((c) => c.name.toLowerCase().includes(needle));
  return (
    <div className="conn-pick">
      <div className="conn-pick-bar">
        <input
          type="search"
          value={filter}
          placeholder={t("Filter connections…")}
          aria-label={t("Filter connections…")}
          onChange={(e) => setFilter(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && e.preventDefault()}
        />
        <span className="muted small">{t("{n} selected", { n: picked.size })}</span>
      </div>
      <div className="conn-pick-list">
        {shown.map((c) => (
          <label key={c.id} className="check">
            <input type="checkbox" checked={picked.has(c.id)} onChange={() => onChange(toggled(picked, c.id))} />
            <DriverIcon driver={c.driver} />
            {c.name}
          </label>
        ))}
        {shown.length === 0 && <span className="muted small conn-pick-empty">{t("Nothing matches.")}</span>}
      </div>
    </div>
  );
}
