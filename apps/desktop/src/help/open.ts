import { workbench } from "../workbench";

/** The dialogs and the text of the guide are a chunk of their own, loaded at first use: the app
 *  does not read them at each start. */
const dialogs = (open: (loaded: typeof import("./HelpDialogs")) => void) =>
  import("./HelpDialogs").then(open).catch((e: unknown) => workbench().showError(String(e)));

/** Opens the guide that comes with the app, on `page` (a key such as `connect`) at the heading `anchor`. */
export const openGuide = (page?: string, anchor: string | null = null) => dialogs((d) => d.openGuide(page, anchor));

/** Opens the changelog section of the running version. */
export const openWhatsNew = () => dialogs((d) => d.openWhatsNew());
