import { useId, useRef, useState } from "react";
import { addTags } from "./annotation";

interface Props {
  tags: string[];
  /** Tags already used on this connection, offered while typing. */
  suggestions: string[];
  /** Resolves `false` when the change was not saved: the typed text then stays in the field. */
  onChange: (tags: string[]) => void | Promise<boolean>;
  label?: string;
}

/** Tags as chips; Enter, comma or leaving the field adds what is typed, a comma-separated list too. */
export function TagInput({ tags, suggestions, onChange, label = "Tags" }: Props) {
  const [draft, setDraft] = useState("");
  const adding = useRef<string | null>(null);
  const list = useId();

  async function commit(text: string) {
    if (adding.current === text) return;
    const next = addTags(tags, text);
    if (next.length === tags.length) {
      setDraft("");
      return;
    }
    // What is typed stays until it is saved, so a failed save leaves it to try again.
    setDraft(text);
    adding.current = text;
    const saved = await Promise.resolve(onChange(next)).catch(() => false);
    adding.current = null;
    if (saved !== false) setDraft((now) => (now === text ? "" : now));
  }

  return (
    <div className="tag-input" aria-label={label}>
      {tags.map((t) => (
        <span key={t} className="tag-chip">
          #{t}
          <button type="button" aria-label={`Remove ${t}`} onClick={() => void onChange(tags.filter((x) => x !== t))}>
            ×
          </button>
        </span>
      ))}
      <input
        value={draft}
        list={list}
        placeholder={tags.length ? "" : "Add a tag"}
        onChange={(e) => (e.target.value.includes(",") ? void commit(e.target.value) : setDraft(e.target.value))}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            void commit(draft);
          } else if (e.key === "Backspace" && !draft && tags.length) {
            void onChange(tags.slice(0, -1));
          }
        }}
        onBlur={() => void commit(draft)}
      />
      <datalist id={list}>
        {suggestions.filter((s) => !tags.includes(s)).map((s) => (
          <option key={s} value={s} />
        ))}
      </datalist>
    </div>
  );
}
