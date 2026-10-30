import { useCallback, useEffect, useRef } from "react";
import { type PointerDragOptions, type Press, pointerDrag } from "./pointerDrag";

/** `pointerDrag` for a component: a drag still going when the component unmounts (or when
 *  it starts another) is cancelled, so its label and listeners never outlive it. */
export function useDrag() {
  const cancel = useRef<(() => void) | null>(null);
  useEffect(() => () => cancel.current?.(), []);
  return useCallback(<T>(press: Press, options: PointerDragOptions<T>) => {
    cancel.current?.();
    cancel.current = pointerDrag(press, options);
  }, []);
}
