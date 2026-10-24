import { Fragment, useEffect, useLayoutEffect, useRef, useState } from "react";
import { type Action, type ActionScope, type ActionScopes, actionsFor, runAction, shortcutLabel } from "../actions";

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

  function choose(action: Action) {
    onClose();
    runAction(action, arg, onError);
  }

  if (actions.length === 0) return null;
  return (
    <div
      ref={ref}
      className="context-menu"
      style={pos}
      onMouseDown={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
    >
      {lines.map((entry, i) => (
        <Fragment key={"action" in entry ? entry.action.id : `submenu:${entry.submenu}`}>
          {i > 0 && groupOf(entry) !== groupOf(lines[i - 1]!) && <hr />}
          {"action" in entry ? (
            <button onClick={() => choose(entry.action)} onMouseEnter={() => setOpen(null)}>
              <span>{entry.action.title}</span>
              {shortcutLabel(entry.action) && <span className="shortcut">{shortcutLabel(entry.action)}</span>}
            </button>
          ) : (
            <div className="submenu-anchor" onMouseEnter={() => setOpen(entry.submenu)}>
              <button onClick={() => setOpen(entry.submenu)} aria-expanded={open === entry.submenu}>
                <span>{entry.submenu}</span>
                <span className="shortcut">▸</span>
              </button>
              {open === entry.submenu && (
                <div className="context-menu submenu">
                  {entry.actions.map((action) => (
                    <button key={action.id} onClick={() => choose(action)}>
                      <span>{action.title}</span>
                      {shortcutLabel(action) && <span className="shortcut">{shortcutLabel(action)}</span>}
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
