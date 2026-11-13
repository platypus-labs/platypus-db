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

/** A key press, as the functions below read it (`KeyboardEvent` has all of it). */
interface KeyPress {
  key: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
  getModifierState?: (key: string) => boolean;
}

/**
 * Whether a key press types a character with AltGr (`[`, `]`, `@`, `#`, `€`, `{`, `\`, `|`, `~`
 * on Italian, German, Spanish, French... keyboards): text, never a shortcut. Windows reports
 * AltGr as Ctrl+Alt, often without the AltGraph state, so there Ctrl+Alt with a character other
 * than a Latin letter or digit counts as AltGr: those come from the plain key (Ctrl+Alt+D on a
 * layout without an AltGr character on D). macOS has no AltGr: Option is Alt.
 */
export function altGraphText(e: KeyPress, mac: boolean): boolean {
  if (mac || [...e.key].length !== 1 || e.key === " " || /^[a-z0-9]$/i.test(e.key)) return false;
  return !!e.getModifierState?.("AltGraph") || (e.ctrlKey && e.altKey && !e.metaKey);
}

/** Whether a shortcut's key is a character some keyboards type with Shift (`/` is Shift+7 on
 *  Italian and German ones, digits on French ones): its key press already says which character
 *  it is, so the shortcut also takes it with Shift held. A letter is not: Shift makes it another
 *  shortcut. */
export function shiftTyped(key: string): boolean {
  const k = sameKey(key);
  return k === "plus" || k === "comma" || ([...k].length === 1 && k !== " " && !/\p{L}/u.test(k));
}

/**
 * Whether key press `e` is `shortcut` (`Mod+Shift+K`). `loose` also takes a Shift the shortcut
 * does not name when its key is `shiftTyped`: callers try the exact match first, so `Mod+/`
 * and `Mod+Shift+/` stay two shortcuts where both exist. An AltGr character is never one.
 */
export function matchesShortcut(shortcut: string, e: KeyPress, mac: boolean, loose = false): boolean {
  if (altGraphText(e, mac)) return false;
  const parts = shortcut.split("+");
  const key = parts.pop()!;
  const mods = new Set(parts.map((p) => p.toLowerCase()));
  const shift = e.shiftKey === mods.has("shift") || (loose && e.shiftKey && shiftTyped(key));
  return (
    // `+` and `,` separate keys and shortcuts, so those keys are written `Plus` and `Comma`.
    sameKey(e.key) === sameKey(key) &&
    (mac ? e.metaKey : e.ctrlKey) === mods.has("mod") &&
    shift &&
    e.altKey === mods.has("alt") &&
    (mac ? e.ctrlKey === mods.has("ctrl") : true)
  );
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
    if (p === "Mod") return name("Ctrl");
    if (p === "Comma") return ",";
    if (p === "Plus") return "+";
    return name(p.replace(/^Arrow/, ""));
  });
}

/** A CodeMirror key binding, as far as `withoutKeys` reads it. */
interface Binding {
  key?: string;
  mac?: string;
  win?: string;
  linux?: string;
  run?: unknown;
  shift?: unknown;
}

/**
 * `bindings` without those on a key of `taken` (as `bindingKey` writes them) or on an AltGr
 * character (`onAltGraph`). A binding whose Shift variant (`shift`) is taken loses that variant;
 * one whose own key is taken keeps the Shift variant alone.
 */
export function withoutKeys<B extends Binding>(bindings: readonly B[], taken: ReadonlySet<string>, mac: boolean, win: boolean): B[] {
  const out: B[] = [];
  for (const b of bindings) {
    const key = (mac ? b.mac : win ? b.win : b.linux) ?? b.key;
    if (!key) {
      out.push(b);
      continue;
    }
    if (onAltGraph(key, mac)) continue;
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

const CHARS: Record<string, string> = { plus: "+", comma: ",", space: " " };

/** Whether a binding (either spelling) is Ctrl+Alt with a character AltGr types, such as
 *  CodeMirror's `Ctrl-Alt-[` (Fold all) or `Mod-Alt-\`: outside macOS it would take that
 *  character from the editor of an Italian or German keyboard. */
export function onAltGraph(binding: string, mac: boolean): boolean {
  const parts = bindingKey(binding, mac).split("+");
  const key = parts.pop()!;
  const press = { key: CHARS[key] ?? key, ctrlKey: true, altKey: true, shiftKey: false, metaKey: parts.includes("meta") };
  return parts.includes("ctrl") && parts.includes("alt") && altGraphText(press, mac);
}

/** Whether a key press while recording removes the shortcut: only a bare Backspace or
 *  Delete; with a modifier held it is a combination to record. */
export function removesShortcut(e: { key: string; ctrlKey: boolean; altKey: boolean; shiftKey: boolean; metaKey: boolean }): boolean {
  return (e.key === "Backspace" || e.key === "Delete") && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey;
}
