/** The toasts of the app: a module-level store, so any code (a React component or an action
 *  module) can call `toast(message, kind)`; `<Toaster />` shows them. The message is already
 *  translated. "ok" and "info" go by themselves; an "error" stays until it is clicked. */
type ToastKind = "ok" | "info" | "error";

interface ToastItem {
  id: number;
  message: string;
  kind: ToastKind;
}

const MAX = 3;
/** How long a toast stays, in ms: an info message is longer, so it can be read. */
const LIFETIME_MS = { ok: 2000, info: 4000 };

let items: ToastItem[] = [];
let nextId = 1;
const listeners = new Set<() => void>();

function set(next: ToastItem[]) {
  items = next;
  listeners.forEach((l) => l());
}

export function dismissToast(id: number) {
  set(items.filter((i) => i.id !== id));
}

/** Shows `message`; the oldest goes when more than three are showing. The same message
 *  shown again replaces the one on screen. */
export function toast(message: string, kind: ToastKind = "ok") {
  const id = nextId++;
  set([...items.filter((i) => i.message !== message || i.kind !== kind), { id, message, kind }].slice(-MAX));
  if (kind !== "error") setTimeout(() => dismissToast(id), LIFETIME_MS[kind]);
}

export function subscribeToasts(listener: () => void) {
  listeners.add(listener);
  return () => void listeners.delete(listener);
}

export const getToasts = () => items;
