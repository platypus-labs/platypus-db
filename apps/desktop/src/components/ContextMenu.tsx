import { Fragment, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { type Action, type ActionScope, type ActionScopes, actionsFor, runAction, shortcutLabel } from "../actions";
import { t } from "../i18n";
import { Icon } from "../ui/Icon";
import { submenuPlacement } from "./menuPlacement";

/** Where a context menu was opened: the scope whose actions it lists, and their argument. */
export type MenuTarget = { [S in ActionScope]: { scope: S; arg: ActionScopes[S] } }[ActionScope];

export interface MenuState {
  x: number;
  y: number;
  target: MenuTarget;
}

interface Props {
  menu: MenuState;
  onClose: () => void;
  onError: (e: unknown) => void;
}

/** A menu line: an action, or a submenu holding the actions that name it. */
type Entry = { action: Action } | { submenu: string; group?: string; actions: Action[] };

function entries(actions: Action[]): Entry[] {
  const out: Entry[] = [];
  const submenus = new Map<string, Action[]>();
  for (const action of actions) {
    if (!action.submenu) {
      out.push({ action });
      continue;
    }
    const existing = submenus.get(action.submenu);
    if (existing) existing.push(action);
    else {
      const list = [action];
      submenus.set(action.submenu, list);
      out.push({ submenu: action.submenu, group: action.group, actions: list });
    }
  }
  return out;
}

/** A submenu kept inside the window: it flips or shifts from its anchor row. It is a fixed
 *  panel in window coordinates, so a menu that scrolls (taller than the window) cannot clip it. */
function Submenu({ children }: { children: React.ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<React.CSSProperties>({ visibility: "hidden" });
  useLayoutEffect(() => {
    const el = ref.current;
    const anchor = el?.parentElement;
    if (!el || !anchor) return;
    // A parent with backdrop-filter is the containing block of a fixed child: its origin is
    // not the window's, so measure where left/top 0 land and subtract that.
    el.style.left = "0";
    el.style.top = "0";
    const { width, height, left: originX, top: originY } = el.getBoundingClientRect();
    const p = submenuPlacement(anchor.getBoundingClientRect(), { width, height }, { width: window.innerWidth, height: window.innerHeight });
    const a = anchor.getBoundingClientRect();
    setPos({ left: a.left + p.left - originX, top: a.top + p.top - originY, ...(p.maxHeight && { maxHeight: p.maxHeight, overflowY: "auto" }) });
  }, []);
  return (
    <div ref={ref} className="context-menu submenu" role="menu" style={pos}>
      {children}
    </div>
  );
}

const groupOf = (e: Entry) => ("action" in e ? e.action.group : e.group);

/** The registered actions of a scope as a popup menu, sections separated by lines. */
export function ContextMenu({ menu, onClose, onError }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<React.CSSProperties>({ left: menu.x, top: menu.y });
  const [open, setOpen] = useState<string | null>(null);
  const id = useId();
  const { scope, arg } = menu.target as { scope: ActionScope; arg: never };
  const actions = actionsFor(scope, arg).filter((a) => !a.hidden);
  const lines = entries(actions);

  /** Closed from the keyboard or by an item: the focus goes back where it was. A click
   *  elsewhere leaves it where the click put it. */
  const giveBack = useRef(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      giveBack.current = true;
      onClose();
    };
    window.addEventListener("mousedown", onClose);
    window.addEventListener("keydown", onKey);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("mousedown", onClose);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);

  // Keep the menu inside the window.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    // Taller than the window (a small window, a large interface size): it scrolls, so every
    // item can be reached. Its submenus are fixed panels, which the scrolling does not clip
    // (the class drops the glass: a backdrop-filter would make the menu their containing block).
    const room = window.innerHeight - 8;
    setPos({
      left: Math.max(0, Math.min(menu.x, window.innerWidth - width - 4)),
      top: Math.max(0, Math.min(menu.y, window.innerHeight - height - 4)),
      ...(height > room && { maxHeight: room, overflowY: "auto" }),
    });
  }, [menu.x, menu.y]);

  // From the keyboard: the focus goes to the first item, and back where it was on closing.
  useEffect(() => {
    const opener = document.activeElement;
    ref.current?.querySelector<HTMLElement>("[role^=menuitem]")?.focus();
    return () => {
      // Not when what the menu ran took the focus, e.g. a dialog it opened.
      const lost = document.activeElement === document.body || document.activeElement === null;
      if (giveBack.current && lost && opener instanceof HTMLElement && opener.isConnected) opener.focus({ preventScroll: true });
    };
  }, []);

  /** Up and down through the items of the menu (or submenu) the focus is in; right opens a
   *  submenu, left goes back from it. */
  function onKey(e: React.KeyboardEvent<HTMLDivElement>) {
    const item = e.target as HTMLElement;
    const list = item.closest(".context-menu");
    if (!list) return;
    const items = [...list.querySelectorAll<HTMLElement>(":scope > [role^=menuitem], :scope > .submenu-anchor > [role^=menuitem]")];
    const at = items.indexOf(item);
    if (e.key === "ArrowDown") items[(at + 1) % items.length]?.focus();
    else if (e.key === "ArrowUp") items[(at - 1 + items.length) % items.length]?.focus();
    else if (e.key === "Home") items[0]?.focus();
    else if (e.key === "End") items.at(-1)?.focus();
    else if (e.key === "ArrowRight" && item.getAttribute("aria-haspopup")) {
      item.click();
      requestAnimationFrame(() => item.parentElement?.querySelector<HTMLElement>(".submenu [role^=menuitem]")?.focus());
    } else if (e.key === "ArrowLeft" && list.classList.contains("submenu")) {
      const anchor = list.parentElement?.querySelector<HTMLElement>(":scope > [role^=menuitem]");
      setOpen(null);
      anchor?.focus();
    } else return;
    e.preventDefault();
    e.stopPropagation();
  }

  function choose(action: Action) {
    if (action.disabled?.(arg)) return;
    giveBack.current = true;
    onClose();
    runAction(action, arg, onError);
  }

  /** An action's item: a disabled one stays focusable, its reason as the tooltip and as its
   *  description for screen readers. */
  function item(action: Action, props?: React.ButtonHTMLAttributes<HTMLButtonElement>) {
    const reason = action.disabled?.(arg) ?? null;
    const checked = action.checked?.(arg);
    const hint = `${id}${action.id}`;
    return (
      <button
        key={action.id}
        role={checked === undefined ? "menuitem" : "menuitemcheckbox"}
        aria-checked={checked}
        data-action={action.id}
        aria-disabled={reason ? true : undefined}
        aria-describedby={reason ? hint : undefined}
        title={reason ? t(reason) : undefined}
        onClick={() => choose(action)}
        {...props}
      >
        <span>
          {checked && <Icon name="commit" size={12} className="menu-check" />}
          {t(action.title)}
        </span>
        {shortcutLabel(action) && <kbd className="shortcut">{shortcutLabel(action)}</kbd>}
        {reason && (
          <span id={hint} hidden>
            {t(reason)}
          </span>
        )}
      </button>
    );
  }

  if (actions.length === 0) return null;
  return (
    <div
      ref={ref}
      className={pos.overflowY ? "context-menu scrolls" : "context-menu"}
      role="menu"
      style={pos}
      // A fixed submenu would stay where its row was: scrolling closes it.
      onScroll={() => setOpen(null)}
      onKeyDown={onKey}
      onMouseDown={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
    >
      {lines.map((entry, i) => (
        <Fragment key={"action" in entry ? entry.action.id : `submenu:${entry.submenu}`}>
          {i > 0 && groupOf(entry) !== groupOf(lines[i - 1]!) && <hr />}
          {"action" in entry ? (
            item(entry.action, { onMouseEnter: () => setOpen(null) })
          ) : (
            <div className="submenu-anchor" onMouseEnter={() => setOpen(entry.submenu)}>
              <button role="menuitem" aria-haspopup="menu" onClick={() => setOpen(entry.submenu)} aria-expanded={open === entry.submenu}>
                <span>{t(entry.submenu)}</span>
                <span className="shortcut">
                  <Icon name="chevron-right" />
                </span>
              </button>
              {open === entry.submenu && (
                <Submenu>{entry.actions.map((action) => item(action))}</Submenu>
              )}
            </div>
          )}
        </Fragment>
      ))}
    </div>
  );
}
