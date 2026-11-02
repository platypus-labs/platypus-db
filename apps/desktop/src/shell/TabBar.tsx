import { type CSSProperties, type KeyboardEvent, type MouseEvent, type PointerEvent, Fragment, forwardRef, useState } from "react";
import { titleWithKeys } from "../actions";
import { defineSetting, useSettingsVersion } from "../settings";
import type { TabModel } from "../tabs/registry";
import { Icon, type IconName } from "../ui/Icon";
import { edgeZone, markedTarget } from "../ui/pointerDrag";
import { scrollSideways } from "../ui/scroll";
import { useDrag } from "../ui/useDrag";
import { neighbour } from "./tabOrder";
import { t } from "../i18n";

export const groupTabsSetting = defineSetting<boolean>({
  key: "tabs.group_by_connection",
  section: "Tabs",
  label: "Group tabs by connection",
  description: "Tabs of the same connection sit together under its name; a click on the name folds them.",
  default: false,
  control: { type: "boolean" },
});

/** The order of the bar: pinned tabs first, then the others, by connection when grouped (in
 *  the order each connection's first tab opened). */
export function barOrder(tabs: TabModel[], grouped: boolean): TabModel[] {
  const pinned = tabs.filter((t) => t.pinned);
  const rest = tabs.filter((t) => !t.pinned);
  if (!grouped) return [...pinned, ...rest];
  const connections = [...new Set(rest.map((t) => t.connectionId))];
  return [...pinned, ...connections.flatMap((id) => rest.filter((t) => t.connectionId === id))];
}

interface Props {
  /** In bar order (`barOrder`). */
  tabs: TabModel[];
  activeTab: number | null;
  /** The other tab shown beside the active one, when the editor is split. */
  besideTab: number | null;
  grouped: boolean;
  /** Connections whose group is folded. */
  folded: ReadonlySet<string>;
  onFold: (connectionId: string) => void;
  title: (tab: TabModel) => string;
  /** The title without the connection, which the group already names. */
  shortTitle: (tab: TabModel) => string;
  icon: (tab: TabModel) => IconName;
  color: (connectionId: string) => string | null | undefined;
  connectionName: (connectionId: string) => string;
  onActivate: (tab: TabModel) => void;
  onClose: (tab: TabModel) => void;
  onMenu: (tab: TabModel, x: number, y: number) => void;
  /** `dragged` was dropped before (or, with `after`, behind) `over`. */
  onMove: (dragged: number, over: number, after: boolean) => void;
}

/** Where a dragged tab would land: beside the tab `key`. */
interface Drop {
  key: number;
  after: boolean;
}

/** The tab bar: a tab list the arrows move through, Delete closes and the menu key opens the
 *  tab's menu. */
export const TabBar = forwardRef<HTMLDivElement, Props>(function TabBar(props, ref) {
  // The close button's tooltip names the current shortcut.
  useSettingsVersion();
  const { tabs, activeTab, besideTab, grouped, folded } = props;
  const startDrag = useDrag();
  const [drop, setDrop] = useState<Drop | null>(null);
  const drag = (tab: TabModel) => (e: PointerEvent) =>
    startDrag<Drop>(e, {
      label: props.title(tab),
      target: (el, at) => {
        const over = markedTarget(el, "data-tab");
        const key = over ? Number(over.value) : NaN;
        if (!over || key === tab.key || Number.isNaN(key)) return null;
        const r = over.element.getBoundingClientRect();
        return { key, after: edgeZone(at.x - r.left, r.width, 0.5) === "after" };
      },
      show: setDrop,
      drop: (target) => props.onMove(tab.key, target.key, target.after),
    });
  const focusTab = (tab: TabModel | undefined, bar: HTMLElement) => {
    if (!tab) return;
    props.onActivate(tab);
    bar.querySelector<HTMLElement>(`[data-tab="${tab.key}"]`)?.focus();
  };
  const onKey = (e: KeyboardEvent<HTMLDivElement>, tab: TabModel) => {
    const shown = tabs.filter((t) => !hidden(t));
    const at = shown.indexOf(tab);
    const bar = e.currentTarget.parentElement!;
    if (e.altKey && e.shiftKey && (e.key === "ArrowRight" || e.key === "ArrowLeft")) {
      // The keyboard way to move the tab: one place along the bar.
      const step = e.key === "ArrowRight" ? 1 : -1;
      const next = neighbour(shown, tab.key, step);
      if (next) props.onMove(tab.key, next.key, step > 0);
    } else if (e.key === "ArrowRight") focusTab(shown[(at + 1) % shown.length], bar);
    else if (e.key === "ArrowLeft") focusTab(shown[(at - 1 + shown.length) % shown.length], bar);
    else if (e.key === "Home") focusTab(shown[0], bar);
    else if (e.key === "End") focusTab(shown.at(-1), bar);
    // A pinned tab has no close button and "Close others" keeps it: Delete does too.
    else if (e.key === "Delete") {
      if (!tab.pinned) props.onClose(tab);
    }
    else if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
      const r = e.currentTarget.getBoundingClientRect();
      props.onMenu(tab, r.left, r.bottom);
    } else return;
    e.preventDefault();
  };
  /** A folded group shows only its active tab. */
  const hidden = (tab: TabModel) => grouped && !tab.pinned && folded.has(tab.connectionId) && tab.key !== activeTab && tab.key !== besideTab;
  return (
    <div className="tabs" role="tablist" aria-label={t("Open tabs")} ref={ref} onWheel={scrollSideways}>
      {tabs.map((tab, i) => {
        const prev = tabs[i - 1];
        const header = grouped && !tab.pinned && (!prev || prev.pinned || prev.connectionId !== tab.connectionId);
        const conn = { "--conn": props.color(tab.connectionId) ?? "transparent" } as CSSProperties;
        const count = header ? tabs.filter((o) => !o.pinned && o.connectionId === tab.connectionId).length : 0;
        const isFolded = folded.has(tab.connectionId);
        return (
          <Fragment key={tab.key}>
            {header && (
              <button
                type="button"
                className="tab-group"
                style={conn}
                aria-expanded={!isFolded}
                title={t(isFolded ? "Show the tabs of this connection" : "Fold the tabs of this connection")}
                onClick={() => props.onFold(tab.connectionId)}
              >
                {props.connectionName(tab.connectionId)}
                {isFolded && <span className="tab-count">{count}</span>}
              </button>
            )}
            {!hidden(tab) && (
              <div
                data-tab={tab.key}
                role="tab"
                aria-selected={tab.key === activeTab}
                tabIndex={tab.key === activeTab ? 0 : -1}
                className={`tab${tab.key === activeTab ? " active" : ""}${tab.key === besideTab ? " beside" : ""}${tab.pinned ? " pinned" : ""}${drop?.key === tab.key ? (drop.after ? " drop-after" : " drop-before") : ""}`}
                style={conn}
                title={tab.pinned ? props.title(tab) : undefined}
                onClick={() => props.onActivate(tab)}
                onPointerDown={drag(tab)}
                // The press of the middle button starts the browser's autoscroll when the bar overflows,
                // and that swallows the click: it is cancelled here.
                onMouseDown={(e) => e.button === 1 && e.preventDefault()}
                onAuxClick={(e) => e.button === 1 && props.onClose(tab)}
                onContextMenu={(e: MouseEvent) => {
                  e.preventDefault();
                  // Under the tab, so the menu does not cover it or its neighbour.
                  props.onMenu(tab, e.clientX, e.currentTarget.getBoundingClientRect().bottom);
                }}
                onKeyDown={(e) => onKey(e, tab)}
              >
                <span className="tab-icon">
                  <Icon name={props.icon(tab)} size={13} />
                  {tab.pinned && <Icon name="pin" size={12} className="tab-pin" />}
                </span>
                {tab.pinned ? (
                  <span className="sr-only">{props.title(tab)}</span>
                ) : (
                  <span>{grouped ? props.shortTitle(tab) : props.title(tab)}</span>
                )}
                {!tab.pinned && (
                  <button
                    type="button"
                    className="icon"
                    tabIndex={-1}
                    title={titleWithKeys(t("Close tab"), "tab.close")}
                    aria-label={t("Close tab")}
                    onClick={(e) => {
                      e.stopPropagation();
                      props.onClose(tab);
                    }}
                  >
                    <Icon name="close" />
                  </button>
                )}
              </div>
            )}
          </Fragment>
        );
      })}
    </div>
  );
});
