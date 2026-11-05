import { t } from "../i18n";
import { openGuide } from "./open";

/** A small "?" that opens the guide on `page` (a key such as `connect`), at the heading `anchor`. */
export function HelpLink({ page, anchor = null }: { page: string; anchor?: string | null }) {
  return (
    <button
      type="button"
      className="help-link"
      data-help={page}
      title={t("Open the guide page about this")}
      aria-label={t("Open the guide page about this")}
      onClick={() => openGuide(page, anchor)}
    >
      ?
    </button>
  );
}
