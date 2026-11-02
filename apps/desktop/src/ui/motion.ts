/**
 * The interface's animations: as the system asks (its "reduce motion"), on, reduced to short
 * fades, or off. The choice is the root's `data-motion` (`full`, `reduced` or `none`), which
 * the styles follow (styles/motion.css).
 */
import { defineSetting, getSetting, onSettingsChanged } from "../settings";
import { msg } from "../i18n";

export const animationsSetting = defineSetting<string>({
  key: "appearance.animations",
  section: msg("Appearance"),
  label: "Animations",
  description: "Panels and dialogs opening, tabs appearing. Reduced: short fades only.",
  default: "system",
  control: {
    type: "select",
    options: [
      { value: "system", label: "Like the system" },
      { value: "on", label: "Enabled" },
      { value: "reduced", label: "Reduced" },
      { value: "off", label: "Off" },
    ],
  },
});

const MOTION: Record<string, string> = { on: "full", reduced: "reduced", off: "none" };

const lessMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

function applyMotion() {
  const motion = MOTION[getSetting(animationsSetting)] ?? (lessMotion.matches ? "reduced" : "full");
  const root = document.documentElement;
  // Any setting change calls this: the styles are worked out again only on a new value.
  if (root.dataset.motion !== motion) root.dataset.motion = motion;
}

/** Sets the root's attribute and follows the setting and the system; call once the settings
 *  are loaded. */
export function startMotion() {
  applyMotion();
  onSettingsChanged(applyMotion);
  lessMotion.addEventListener("change", applyMotion);
}
