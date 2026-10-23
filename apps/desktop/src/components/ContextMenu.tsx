import { Fragment, useEffect, useLayoutEffect, useRef, useState } from "react";
import { type ActionScope, type ActionScopes, actionsFor, runAction, shortcutLabel } from "../actions";

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

/** The registered actions of a scope as a popup menu, sections separated by lines. */
export function ContextMenu({ menu, onClose, onError }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: menu.x, top: menu.y });
  const { scope, arg } = menu.target as { scope: ActionScope; arg: never };
  const actions = actionsFor(scope, arg).filter((a) => !a.hidden);

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

  if (actions.length === 0) return null;
  return (
    <div
      ref={ref}
      className="context-menu"
      style={pos}
      onMouseDown={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
    >
      {actions.map((action, i) => (
        <Fragment key={action.id}>
          {i > 0 && action.group !== actions[i - 1]!.group && <hr />}
          <button
            onClick={() => {
              onClose();
              runAction(action, arg, onError);
            }}
          >
            <span>{action.title}</span>
            {shortcutLabel(action) && <span className="shortcut">{shortcutLabel(action)}</span>}
          </button>
        </Fragment>
      ))}
    </div>
  );
}
