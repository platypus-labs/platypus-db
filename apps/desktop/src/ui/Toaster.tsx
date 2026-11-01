import { useSyncExternalStore } from "react";
import { dismissToast, getToasts, subscribeToasts } from "./toast";

/** The toasts (see toast.ts): render it once per window. An error is an alert, the rest are
 *  polite status messages. */
export function Toaster() {
  const items = useSyncExternalStore(subscribeToasts, getToasts);
  return (
    <div className="toaster" role="status" aria-live="polite">
      {items.map((i) => (
        <div
          key={i.id}
          className={`notice ${i.kind === "error" ? "error" : "ok"} toast ${i.kind}`}
          role={i.kind === "error" ? "alert" : undefined}
          onClick={() => dismissToast(i.id)}
        >
          {i.message}
        </div>
      ))}
    </div>
  );
}
