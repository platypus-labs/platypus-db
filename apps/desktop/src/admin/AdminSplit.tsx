import type { CSSProperties, ReactNode } from "react";
import { t } from "../i18n";
import { PanelResizer, useStoredSize } from "../shell/PanelResizer";

/** A list beside its details panel, with a handle that resizes the panel (width kept). */
export function AdminSplit({ list, details }: { list: ReactNode; details: ReactNode }) {
  const [width, setWidth] = useStoredSize("layout.admin.details", 380);
  return (
    <div className="admin-split" style={{ "--admin-details-w": `${width}px` } as CSSProperties}>
      {list}
      <PanelResizer
        storeKey="layout.admin.details"
        size={width}
        fallback={380}
        min={200}
        max={() => Math.max(200, window.innerWidth - 400)}
        direction="column"
        after
        onResize={setWidth}
        label={t("Resize the details")}
      />
      {details}
    </div>
  );
}
