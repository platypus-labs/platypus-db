import { useEffect, useRef } from "react";

/** A ref for a driver popup that takes the keyboard from the form behind it, so that Escape and
 *  Enter do not reach that form, and gives it back when the popup closes. */
export function usePopupFocus() {
  const popup = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    popup.current?.focus();
    return () => before?.focus();
  }, []);
  return popup;
}
