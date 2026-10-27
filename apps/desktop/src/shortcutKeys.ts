/**
 * Shortcuts as text: `Mod+Shift+K`, several separated by commas. `+` and `,` are written
 * `Plus` and `Comma`, since they separate the keys and the shortcuts.
 */

const NAMES: Record<string, string> = { " ": "Space", "+": "Plus", ",": "Comma" };

/** The name a shortcut gives the key of a key press (`KeyboardEvent.key`). */
export function keyName(key: string): string {
  return NAMES[key] ?? (key.length === 1 ? key.toUpperCase() : key);
}

/** A key name or `KeyboardEvent.key`, lowercase, as two spellings of one key compare. */
export function sameKey(key: string): string {
  return (NAMES[key] ?? key).toLowerCase();
}

/** The shortcuts of a stored list. A comma key written as itself (`Mod+,`, as stored before
 *  it had a name) is kept, not taken for a separator. */
export function splitShortcuts(list: string): string[] {
  if (!list.trim()) return [];
  return list.replace(/\+,(?=\s*(?:,|$))/g, "+Comma").split(/\s*,\s*/);
}

/** Whether a combination needs no Ctrl, Alt or Cmd and its key types a character: typing
 *  would run it instead of writing the character. */
export function typesText(shortcut: string): boolean {
  const parts = shortcut.split("+");
  const key = parts.pop()!;
  const mods = parts.filter((p) => p !== "Shift");
  return mods.length === 0 && (key.length === 1 || key === "Space" || key === "Plus" || key === "Comma");
}

/** The parts of a shortcut as shown, for a platform other than macOS: `Mod` is Ctrl, arrows
 *  lose their prefix, named characters show as themselves. `name` translates key names. */
export function labelParts(shortcut: string, name: (key: string) => string = (k) => k): string[] {
  return shortcut.split("+").map((p) => {
    if (p === "Mod") return "Ctrl";
    if (p === "Comma") return ",";
    if (p === "Plus") return "+";
    return name(p.replace(/^Arrow/, ""));
  });
}

/** A CodeMirror key binding, as far as `withoutKeys` reads it. */
export interface Binding {
  key?: string;
  mac?: string;
  win?: string;
  linux?: string;
  run?: unknown;
  shift?: unknown;
}

/**
 * `bindings` without those on a key of `taken` (as `bindingKey` writes them). A binding whose
 * Shift variant (`shift`) is taken loses that variant; one whose own key is taken keeps the
 * Shift variant alone.
 */
export function withoutKeys<B extends Binding>(bindings: readonly B[], taken: ReadonlySet<string>, mac: boolean, win: boolean): B[] {
  const out: B[] = [];
  for (const b of bindings) {
    const key = (mac ? b.mac : win ? b.win : b.linux) ?? b.key;
    if (!key) {
      out.push(b);
      continue;
    }
    const own = taken.has(bindingKey(key, mac));
    const shifted = b.shift !== undefined && taken.has(bindingKey(`Shift-${key}`, mac));
    if (!own) out.push(shifted ? { ...b, shift: undefined } : b);
    else if (b.shift !== undefined && !shifted) {
      out.push({ ...b, key: `Shift-${key}`, mac: undefined, win: undefined, linux: undefined, run: b.shift, shift: undefined });
    }
  }
  return out;
}

/** CodeMirror's spelling of a key binding (`Shift-Mod-k`) or ours (`Mod+Shift+K`), reduced to
 *  one form for comparing: concrete modifiers in a fixed order, then the key. */
export function bindingKey(binding: string, mac: boolean): string {
  const parts = binding.includes("+") && !binding.endsWith("+") ? binding.split("+") : binding.split(/-(?!$)/);
  const key = parts.pop()!;
  const mods = new Set<string>();
  for (const raw of parts) {
    const m = raw.toLowerCase();
    if (m === "mod") mods.add(mac ? "meta" : "ctrl");
    else if (m === "cmd" || m === "meta") mods.add("meta");
    else if (m === "ctrl" || m === "control" || m === "c") mods.add("ctrl");
    else if (m === "alt" || m === "a") mods.add("alt");
    else if (m === "shift" || m === "s") mods.add("shift");
  }
  return [...["ctrl", "meta", "alt", "shift"].filter((m) => mods.has(m)), sameKey(key)].join("+");
}
