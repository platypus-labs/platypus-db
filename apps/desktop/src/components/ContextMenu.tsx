import { Fragment, useEffect, useLayoutEffect, useRef, useState } from "react";
import { type Action, type ActionScope, type ActionScopes, actionsFor, runAction, shortcutLabel } from "../actions";
import { t } from "../i18n";

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

const groupOf = (e: Entry) => ("action" in e ? e.action.group : e.group);

/** The registered actions of a scope as a popup menu, sections separated by lines. */
export function ContextMenu({ menu, onClose, onError }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: menu.x, top: menu.y });
  const [open, setOpen] = useState<string | null>(null);
  const { scope, arg } = menu.target as { scope: ActionScope; arg: never };
  const actions = actionsFor(scope, arg).filter((a) => !a.hidden);
  const lines = entries(actions);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
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
    setPos({
      left: Math.max(0, Math.min(menu.x, window.innerWidth - width - 4)),
      top: Math.max(0, Math.min(menu.y, window.innerHeight - height - 4)),
    });
  }, [menu.x, menu.y]);

  // From the keyboard: the focus goes to the first item, and back where it was on closing.
  useEffect(() => {
    const opener = document.activeElement;
    ref.current?.querySelector<HTMLElement>("[role=menuitem]")?.focus();
    return () => {
      // Not when what the menu ran took the focus, e.g. a dialog it opened.
      const lost = document.activeElement === document.body || document.activeElement === null;
      if (lost && opener instanceof HTMLElement && opener.isConnected) opener.focus({ preventScroll: true });
    };
  }, []);

  /** Up and down through the items of the menu (or submenu) the focus is in; right opens a
   *  submenu, left goes back from it. */
  function onKey(e: React.KeyboardEvent<HTMLDivElement>) {
    const item = e.target as HTMLElement;
    const list = item.closest(".context-menu");
    if (!list) return;
    const items = [...list.querySelectorAll<HTMLElement>(":scope > [role=menuitem], :scope > .submenu-anchor > [role=menuitem]")];
    const at = items.indexOf(item);
    if (e.key === "ArrowDown") items[(at + 1) % items.length]?.focus();
    else if (e.key === "ArrowUp") items[(at - 1 + items.length) % items.length]?.focus();
    else if (e.key === "Home") items[0]?.focus();
    else if (e.key === "End") items.at(-1)?.focus();
    else if (e.key === "ArrowRight" && item.getAttribute("aria-haspopup")) {
      item.click();
      requestAnimationFrame(() => item.parentElement?.querySelector<HTMLElement>(".submenu [role=menuitem]")?.focus());
    } else if (e.key === "ArrowLeft" && list.classList.contains("submenu")) {
      const anchor = list.parentElement?.querySelector<HTMLElement>(":scope > [role=menuitem]");
      setOpen(null);
      anchor?.focus();
    } else return;
    e.preventDefault();
    e.stopPropagation();
  }

  function choose(action: Action) {
    onClose();
    runAction(action, arg, onError);
  }

  if (actions.length === 0) return null;
  return (
    <div
      ref={ref}
      className="context-menu"
      role="menu"
      style={pos}
      onKeyDown={onKey}
      onMouseDown={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
    >
      {lines.map((entry, i) => (
        <Fragment key={"action" in entry ? entry.action.id : `submenu:${entry.submenu}`}>
          {i > 0 && groupOf(entry) !== groupOf(lines[i - 1]!) && <hr />}
          {"action" in entry ? (
            <button role="menuitem" onClick={() => choose(entry.action)} onMouseEnter={() => setOpen(null)}>
              <span>{t(entry.action.title)}</span>
              {shortcutLabel(entry.action) && <kbd className="shortcut">{shortcutLabel(entry.action)}</kbd>}
            </button>
          ) : (
            <div className="submenu-anchor" onMouseEnter={() => setOpen(entry.submenu)}>
              <button role="menuitem" aria-haspopup="menu" onClick={() => setOpen(entry.submenu)} aria-expanded={open === entry.submenu}>
                <span>{t(entry.submenu)}</span>
                <span className="shortcut">▸</span>
              </button>
              {open === entry.submenu && (
                <div className="context-menu submenu" role="menu">
                  {entry.actions.map((action) => (
                    <button key={action.id} role="menuitem" onClick={() => choose(action)}>
                      <span>{t(action.title)}</span>
                      {shortcutLabel(action) && <kbd className="shortcut">{shortcutLabel(action)}</kbd>}
                    </button>
                  ))}
                </div>
              )}
            </div>
          )}
        </Fragment>
      ))}
    </div>
  );
}
