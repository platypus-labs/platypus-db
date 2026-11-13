/**
 * The icon family of the whole app, chosen in Settings › Appearance as cards that show a few
 * of its icons. `Icon` reads the key itself; the logos of the databases stay as they are.
 */
import { defineSetting } from "../settings";
import { msg } from "../i18n";
import { ICON_SETS, type IconSet } from "./icons/sets";

const ABOUT: Record<IconSet, string> = {
  lucide: msg("The default: a medium stroke, soft corners."),
  tabler: msg("A thinner stroke, lighter on the screen."),
  phosphor: msg("Rounder shapes, easy to tell apart when small."),
};

defineSetting<string>({
  key: "appearance.icon_set",
  section: msg("Appearance"),
  label: "Icon set",
  description: "The whole app changes together; the logos of the databases stay as they are.",
  default: "lucide",
  control: {
    type: "pictures",
    // The families' names are proper names: not translated.
    options: ICON_SETS.map((s) => ({ value: s.id, label: s.label, description: ABOUT[s.id], picture: { icons: s.id } })),
  },
});
