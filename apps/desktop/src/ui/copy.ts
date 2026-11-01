import { t } from "../i18n";
import { toast } from "./toast";

/** Writes `text` to the clipboard and confirms with a toast (`what` names what was copied).
 *  Resolves to whether the text was written; it never rejects. */
export async function copyText(text: string, what?: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    toast(what ? t("Copied: {what}", { what }) : t("Copied"));
    return true;
  } catch {
    toast(t("Could not copy to the clipboard."), "error");
    return false;
  }
}
