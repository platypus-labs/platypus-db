import { type PointerEvent } from "react";
import { t } from "../i18n";
import { SIDEBAR_MIN, clampSidebar, setSidebarWidth } from "./sidebar";

/** The drag handle on the sidebar's right edge: a drag resizes, a double click restores the
 *  default width, the arrows resize from the keyboard. */
export function SidebarHandle({ width }: { width: number }) {
  const onDown = (e: PointerEvent<HTMLDivElement>) => {
    const handle = e.currentTarget;
    const shell = handle.closest<HTMLElement>(".shell")!;
    const start = { x: e.clientX, width };
    let latest = width;
    handle.setPointerCapture(e.pointerId);
    handle.classList.add("dragging");
    const move = (m: globalThis.PointerEvent) => {
      latest = clampSidebar(start.width + m.clientX - start.x);
      shell.style.setProperty("--sidebar-w", `${latest}px`);
    };
    const up = () => {
      handle.classList.remove("dragging");
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", up);
      handle.removeEventListener("pointercancel", up);
      if (latest !== start.width) setSidebarWidth(latest);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
    handle.addEventListener("pointercancel", up);
  };
  return (
    <div
      className="sidebar-handle"
      role="separator"
      aria-orientation="vertical"
      aria-label={t("Resize the sidebar")}
      aria-valuenow={width}
      aria-valuemin={SIDEBAR_MIN}
      tabIndex={0}
      title={t("Drag to resize, double-click to reset")}
      onPointerDown={onDown}
      onDoubleClick={() => setSidebarWidth(null)}
      onKeyDown={(e) => {
        const step = e.key === "ArrowLeft" ? -16 : e.key === "ArrowRight" ? 16 : 0;
        if (!step) return;
        e.preventDefault();
        setSidebarWidth(width + step);
      }}
    />
  );
}
