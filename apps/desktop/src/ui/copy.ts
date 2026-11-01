import { t } from "../i18n";
import { toast } from "./toast";

/** Writes `text` to the clipboard and confirms with a toast (`what` names what was copied). */
export async function copyText(text: string, what?: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast(what ? t("Copied: {what}", { what }) : t("Copied"));
  } catch {
    toast(t("Could not copy to the clipboard."));
  }
}
