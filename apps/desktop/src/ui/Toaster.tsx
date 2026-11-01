import { useSyncExternalStore } from "react";
import { dismissToast, getToasts, subscribeToasts } from "./toast";

/** The confirmation toasts (see toast.ts): render it once per window. */
export function Toaster() {
  const items = useSyncExternalStore(subscribeToasts, getToasts);
  return (
    <div className="toaster" role="status" aria-live="polite">
      {items.map((i) => (
        <div key={i.id} className="notice ok toast-item" onClick={() => dismissToast(i.id)}>
          {i.message}
        </div>
      ))}
    </div>
  );
}
