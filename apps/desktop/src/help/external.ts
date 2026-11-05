import { api, toCommandError } from "../api";
import { workbench } from "../workbench";

/** The end-to-end tests cannot see the system browser: with `capture` on, the addresses a click
 *  would open are collected here instead. Switched on only with devtools (App.tsx). */
export const externalLinks = { capture: false, urls: [] as string[] };

/** Opens a web or mail address in the system's browser or mail client. */
export async function openExternal(url: string): Promise<void> {
  if (externalLinks.capture) {
    externalLinks.urls.push(url);
    return;
  }
  try {
    await api.openExternalUrl(url);
  } catch (e) {
    workbench().showError(toCommandError(e).message);
  }
}
