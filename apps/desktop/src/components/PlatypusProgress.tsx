// The app's progress bar: the platypus mascot walks along the track, humming, and says
// something useful while the wait lasts. Without `value` and `max` it walks back and forth.
import { useEffect, useState } from "react";
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
          {layer("head", head)}
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
          <span className="platypus-notes" aria-hidden="true">
            ♪ ♫
          </span>
          <Platypus />
        </div>
      </div>
    </div>
  );
}
