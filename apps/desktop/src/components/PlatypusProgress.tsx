// The app's progress bar: the platypus mascot walks along the track, humming, and says
// something useful while the wait lasts. Without `value` and `max` it walks back and forth.
import { useEffect, useRef, useState } from "react";
import { t } from "../i18n";
import arm from "../assets/brand/parts/arm.png";
import body from "../assets/brand/parts/body.png";
import bottom from "../assets/brand/parts/bottom.png";
import footLeft from "../assets/brand/parts/foot-left.png";
import footRight from "../assets/brand/parts/foot-right.png";
import head from "../assets/brand/parts/head.png";
import tail from "../assets/brand/parts/tail.png";

/** How long each tip stays up. */
const TIP_SECONDS = 6;

function tips(says: string[]): string[] {
  return [
    ...says,
    t("Drivers are downloaded once and shared by every connection of that database."),
    t("The command palette finds any action by its name."),
    t("Ctrl+Enter runs the statement under the cursor."),
    t("Production connections commit by hand, so nothing is saved by accident."),
    t("Saved passwords go to the system keychain."),
    t("Idle connections are checked every minute and reopened if the server dropped them."),
    t("♪ La la la… I walk as fast as the network lets me."),
    t("Almost there. Probably."),
    t("Fun fact: a platypus hunts with its eyes shut, feeling for electric signals with its bill."),
    t("Have a sip of water while you wait."),
  ];
}

/** A hat now and then drops onto its head, stays a while and is taken off, like Agent P's. */
type Hat = "off" | "drop" | "on" | "doff";

const between = (min: number, max: number) => min + Math.random() * (max - min);

function useHat(enabled: boolean): Hat {
  const [hat, setHat] = useState<Hat>("off");
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => {
    if (!enabled) return;
    // Each step says how long it lasts and what comes next.
    const steps: Record<Hat, () => [number, Hat]> = {
      off: () => [between(8000, 20000), "drop"],
      drop: () => [1100, "on"],
      on: () => [between(3500, 7000), "doff"],
      doff: () => [1700, "off"],
    };
    const run = (state: Hat) => {
      setHat(state);
      const [wait, next] = steps[state]();
      timer.current = setTimeout(() => run(next), wait);
    };
    run("off");
    return () => clearTimeout(timer.current);
  }, [enabled]);
  return hat;
}

/** The mascot in the parts of docs/brand/parts, stacked back to front on one canvas: the
 *  feet step, the rest bobs, the tail wags and the arm swings (platypus-progress.css). */
function Platypus() {
  const layer = (name: string, src: string) => <img className={`platypus-part platypus-${name}`} src={src} alt="" draggable={false} />;
  return (
    <span className="platypus-sprite" aria-hidden="true">
      <span className="platypus-canvas">
        {layer("foot-r", footRight)}
        {layer("foot-l", footLeft)}
        <span className="platypus-bob">
          {layer("tail", tail)}
          {layer("bottom", bottom)}
          {layer("body", body)}
          <span className="platypus-headwrap">
            {layer("head", head)}
            <svg className="platypus-hat" viewBox="0 0 64 44" aria-hidden="true" focusable="false">
              <ellipse className="hat-brim" cx="32" cy="36" rx="31" ry="6.5" />
              <path className="hat-crown" d="M13 36 C11.5 23 14 12 21 8.5 C26 6 38 6 43 8.5 C50 12 52.5 23 51 36 Z" />
              <path className="hat-band" d="M12.4 31 Q32 37.5 51.6 31 L51.2 36 Q32 41.5 12.8 36 Z" />
              <path className="hat-dent" d="M22 9.5 Q32 14.5 42 9.5" />
              <path className="hat-light" d="M17.5 30 C16.5 22 18 14 22.5 11" />
            </svg>
          </span>
          {layer("arm", arm)}
        </span>
      </span>
    </span>
  );
}

interface Props {
  /** Work done and total, in any unit; both missing means no known end. */
  value?: number;
  max?: number | null;
  label: string;
  /** Only the walk, for a bar that shares a row with other controls. */
  compact?: boolean;
  /** What it says first, before the general tips: for a wait that has its own. */
  says?: string[];
}

export function PlatypusProgress({ value, max, label, compact = false, says = [] }: Props) {
  const known = max != null && max > 0 && value !== undefined;
  const fraction = known ? Math.min(1, Math.max(0, value / max)) : 0;
  const [tip, setTip] = useState(0);
  const hat = useHat(!compact);

  useEffect(() => {
    if (compact) return;
    const timer = setInterval(() => setTip((n) => n + 1), TIP_SECONDS * 1000);
    return () => clearInterval(timer);
  }, [compact]);

  const all = compact ? [] : tips(says);
  return (
    <div
      className={`platypus-progress${compact ? " compact" : ""}${known ? "" : " indeterminate"}${known && fraction >= 1 ? " done" : ""}`}
      role="progressbar"
      aria-label={label}
      aria-valuemin={known ? 0 : undefined}
      aria-valuemax={known ? max : undefined}
      aria-valuenow={known ? value : undefined}
      data-hat={hat}
      style={{ "--platypus-fill": fraction } as React.CSSProperties}
    >
      {!compact && all.length > 0 && (
        <p className="platypus-tip small" key={tip % all.length} aria-hidden="true">
          {all[tip % all.length]}
        </p>
      )}
      <div className="platypus-track">
        <div className="platypus-fill" />
        <div className="platypus-walker">
          <span className="platypus-puff" aria-hidden="true" />
          <span className="platypus-puff" aria-hidden="true" />
          <span className="platypus-spark" aria-hidden="true">
            ✦
          </span>
          <span className="platypus-notes" aria-hidden="true">
            ♪ ♫
          </span>
          <Platypus />
        </div>
      </div>
    </div>
  );
}
