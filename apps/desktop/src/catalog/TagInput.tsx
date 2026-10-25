import { useId, useState } from "react";

interface Props {
  tags: string[];
  /** Tags already used on this connection, offered while typing. */
  suggestions: string[];
  onChange: (tags: string[]) => void;
  label?: string;
}

/** Tags as chips; Enter, comma or leaving the field adds the typed one. */
export function TagInput({ tags, suggestions, onChange, label = "Tags" }: Props) {
  const [draft, setDraft] = useState("");
  const list = useId();

  function commit() {
    const tag = draft.trim().replace(/^#/, "").toLowerCase().split(/\s+/).join("-");
    setDraft("");
    if (tag && !tags.includes(tag)) onChange([...tags, tag].sort());
  }

  return (
    <div className="tag-input" aria-label={label}>
      {tags.map((t) => (
        <span key={t} className="tag-chip">
          #{t}
          <button type="button" aria-label={`Remove ${t}`} onClick={() => onChange(tags.filter((x) => x !== t))}>
            ×
          </button>
        </span>
      ))}
      <input
        value={draft}
        list={list}
        placeholder={tags.length ? "" : "Add a tag"}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === ",") {
            e.preventDefault();
            commit();
          } else if (e.key === "Backspace" && !draft && tags.length) {
            onChange(tags.slice(0, -1));
          }
        }}
        onBlur={commit}
      />
      <datalist id={list}>
        {suggestions.filter((s) => !tags.includes(s)).map((s) => (
          <option key={s} value={s} />
        ))}
      </datalist>
    </div>
  );
}
