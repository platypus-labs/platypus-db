import { type CSSProperties, type KeyboardEvent, type MouseEvent, Fragment, forwardRef } from "react";
import { defineSetting } from "../settings";
import type { TabModel } from "../tabs/registry";
import { Icon, type IconName } from "../ui/Icon";
import { scrollSideways } from "../ui/scroll";

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
}

/** The tab bar: a tab list the arrows move through, Delete closes and the menu key opens the
 *  tab's menu. */
export const TabBar = forwardRef<HTMLDivElement, Props>(function TabBar(props, ref) {
  const { tabs, activeTab, besideTab, grouped, folded } = props;
  const focusTab = (tab: TabModel | undefined, bar: HTMLElement) => {
    if (!tab) return;
    props.onActivate(tab);
    bar.querySelector<HTMLElement>(`[data-tab="${tab.key}"]`)?.focus();
  };
  const onKey = (e: KeyboardEvent<HTMLDivElement>, tab: TabModel) => {
    const shown = tabs.filter((t) => !hidden(t));
    const at = shown.indexOf(tab);
    const bar = e.currentTarget.parentElement!;
    if (e.key === "ArrowRight") focusTab(shown[(at + 1) % shown.length], bar);
    else if (e.key === "ArrowLeft") focusTab(shown[(at - 1 + shown.length) % shown.length], bar);
    else if (e.key === "Home") focusTab(shown[0], bar);
    else if (e.key === "End") focusTab(shown.at(-1), bar);
    else if (e.key === "Delete") props.onClose(tab);
    else if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
      const r = e.currentTarget.getBoundingClientRect();
      props.onMenu(tab, r.left, r.bottom);
    } else return;
    e.preventDefault();
  };
  /** A folded group shows only its active tab. */
  const hidden = (t: TabModel) => grouped && !t.pinned && folded.has(t.connectionId) && t.key !== activeTab && t.key !== besideTab;
  return (
    <div className="tabs" role="tablist" aria-label="Open tabs" ref={ref} onWheel={scrollSideways}>
      {tabs.map((t, i) => {
        const prev = tabs[i - 1];
        const header = grouped && !t.pinned && (!prev || prev.pinned || prev.connectionId !== t.connectionId);
        const conn = { "--conn": props.color(t.connectionId) ?? "transparent" } as CSSProperties;
        const count = header ? tabs.filter((o) => !o.pinned && o.connectionId === t.connectionId).length : 0;
        const isFolded = folded.has(t.connectionId);
        return (
          <Fragment key={t.key}>
            {header && (
              <button
                type="button"
                className="tab-group"
                style={conn}
                aria-expanded={!isFolded}
                title={isFolded ? "Show the tabs of this connection" : "Fold the tabs of this connection"}
                onClick={() => props.onFold(t.connectionId)}
              >
                {props.connectionName(t.connectionId)}
                {isFolded && <span className="tab-count">{count}</span>}
              </button>
            )}
            {!hidden(t) && (
              <div
                data-tab={t.key}
                role="tab"
                aria-selected={t.key === activeTab}
                tabIndex={t.key === activeTab ? 0 : -1}
                className={`tab${t.key === activeTab ? " active" : ""}${t.key === besideTab ? " beside" : ""}${t.pinned ? " pinned" : ""}`}
                style={conn}
                title={t.pinned ? props.title(t) : undefined}
                onClick={() => props.onActivate(t)}
                onAuxClick={(e) => e.button === 1 && props.onClose(t)}
                onContextMenu={(e: MouseEvent) => {
                  e.preventDefault();
                  props.onMenu(t, e.clientX, e.clientY);
                }}
                onKeyDown={(e) => onKey(e, t)}
              >
                <span className="tab-icon">
                  <Icon name={t.pinned ? "pin" : props.icon(t)} size={13} />
                </span>
                {t.pinned ? (
                  <span className="sr-only">{props.title(t)}</span>
                ) : (
                  <span>{grouped ? props.shortTitle(t) : props.title(t)}</span>
                )}
                {!t.pinned && (
                  <button
                    type="button"
                    className="icon"
                    tabIndex={-1}
                    title="Close tab (Ctrl+W)"
                    aria-label="Close tab"
                    onClick={(e) => {
                      e.stopPropagation();
                      props.onClose(t);
                    }}
                  >
                    ×
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
