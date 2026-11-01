/** Confirmation toasts: a module-level store, so any code (a React component or an action
 *  module) can call `toast(message)`; `<Toaster />` shows them. The message is already translated. */
export interface ToastItem {
  id: number;
  message: string;
}

const MAX = 3;
const LIFETIME_MS = 2000;

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

/** Shows `message` for about two seconds; the oldest goes when more than three are showing. */
export function toast(message: string) {
  const id = nextId++;
  set([...items, { id, message }].slice(-MAX));
  setTimeout(() => dismissToast(id), LIFETIME_MS);
}

export function subscribeToasts(listener: () => void) {
  listeners.add(listener);
  return () => void listeners.delete(listener);
}

export const getToasts = () => items;
