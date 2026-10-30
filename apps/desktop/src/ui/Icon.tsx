/**
 * The app's icons, by name. Components use `Icon` and never import an icon family themselves: a
 * new icon is one more name in `icons/lucide.ts` plus its Tabler and Phosphor match, and only the
 * icons named there end up in the bundle. The family comes from the `appearance.icon_set`
 * preference.
 */
import { useSyncExternalStore } from "react";
import { getStored, onSettingsChanged } from "../settings";
import { LUCIDE, type IconName } from "./icons/lucide";
import { ICON_SETS, type IconSet } from "./icons/sets";
import { PHOSPHOR } from "./icons/phosphor";
import { TABLER } from "./icons/tabler";
import type { ObjectKind } from "../api";

export type { IconName };
export { ICON_SETS };
export type { IconSet };

const SET_KEY = "appearance.icon_set";

/** The family chosen in Settings; Lucide when unset or unknown. */
function readIconSet(): IconSet {
  const v = getStored(SET_KEY);
  return ICON_SETS.some((s) => s.id === v) ? (v as IconSet) : "lucide";
}

// One subscription to the settings for all icons: they re-render only when the family changes.
const setListeners = new Set<() => void>();
let lastSet: IconSet | null = null;
let stopWatching: (() => void) | undefined;

function subscribeIconSet(listener: () => void) {
  if (setListeners.size === 0) {
    lastSet = readIconSet();
    stopWatching = onSettingsChanged(() => {
      const now = readIconSet();
      if (now === lastSet) return;
      lastSet = now;
      setListeners.forEach((l) => l());
    });
  }
  setListeners.add(listener);
  return () => {
    setListeners.delete(listener);
    if (setListeners.size === 0) stopWatching?.();
  };
}
const useIconSet = () => useSyncExternalStore(subscribeIconSet, readIconSet);

/**
 * An icon in the current text color; decorative unless it has a `title`. `set` forces a family
 * (the Settings samples), otherwise the chosen one applies.
 */
export function Icon({ name, size = 14, className, title, set }: { name: IconName; size?: number; className?: string; title?: string; set?: IconSet }) {
  const chosen = useIconSet();
  const a11y = { "aria-hidden": title ? undefined : (true as const), "aria-label": title, role: title ? "img" : undefined };
  switch (set ?? chosen) {
    case "tabler": {
      const Svg = TABLER[name];
      return <Svg size={size} stroke={1.75} className={className} {...a11y} />;
    }
    case "phosphor": {
      const Svg = PHOSPHOR[name];
      return <Svg size={size} weight="regular" className={className} {...a11y} />;
    }
    default: {
      const Svg = LUCIDE[name];
      return <Svg size={size} strokeWidth={2} className={className} {...a11y} />;
    }
  }
}

const OBJECT_ICONS: Record<ObjectKind, IconName> = {
  table: "table",
  foreign_table: "table",
  view: "view",
  materialized_view: "view",
  function: "routine",
  procedure: "routine",
  trigger: "trigger",
  sequence: "sequence",
  type: "type",
  domain: "type",
  extension: "extension",
  event: "event",
  dictionary: "table",
  synonym: "view",
  package: "routine",
};

/** The icon of a kind of database object. */
export function objectIcon(kind: ObjectKind): IconName {
  return OBJECT_ICONS[kind];
}
