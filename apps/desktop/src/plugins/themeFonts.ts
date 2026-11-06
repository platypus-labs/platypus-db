// The fonts of the plugin theme in use. A chunk of its own, loaded only when that theme has fonts:
// the bytes come from the package through the backend, go to the browser as a `FontFace`, and the
// family's name is ours, so nothing of the package is ever written as CSS. A font the browser
// refuses is logged and the app's own stays.
import { api } from "../api";
import { THEME_EVENT } from "../theme";

const VARIABLES: Record<string, string> = { ui: "--font-ui", mono: "--font-mono" };

let faces: FontFace[] = [];
/** Newer calls win over a load still running. */
let current = 0;

function clear() {
  const root = document.documentElement;
  for (const face of faces) document.fonts.delete(face);
  faces = [];
  for (const variable of Object.values(VARIABLES)) root.style.removeProperty(variable);
}

/** Uses the fonts `uses` of plugin `id`, or with `null` goes back to the app's. */
export async function applyThemeFonts(id: string | null, uses: string[]) {
  const call = ++current;
  clear();
  if (id !== null) {
    const root = document.documentElement;
    for (const use of uses) {
      const variable = VARIABLES[use];
      if (!variable) continue;
      try {
        const family = `Platypus Theme ${use}`;
        const face = new FontFace(family, await api.pluginThemeFont(id, use));
        await face.load();
        if (call !== current) return;
        document.fonts.add(face);
        faces.push(face);
        // After the app's own stack, read while the variable is not overridden.
        const fallback = getComputedStyle(root).getPropertyValue(variable);
        root.style.setProperty(variable, `"${family}", ${fallback}`);
      } catch (e) {
        console.warn("theme font", id, use, e);
      }
    }
  }
  window.dispatchEvent(new Event(THEME_EVENT));
}
