import { registerAction } from "../actions";
import { setStored } from "../settings";
import { workbench } from "../workbench";
import { openExternal } from "./external";
import { openGuide, openWhatsNew } from "./open";

/** Where the project takes bug reports (the address the README and CONTRIBUTING give). */
const BUG_REPORTS_URL = "https://github.com/platypus-labs/platypus-db/issues";

registerAction({ id: "help.guide", scope: "help", title: "User guide", group: "learn", order: 10, run: () => openGuide() });

registerAction({
  id: "help.shortcuts",
  scope: "help",
  title: "Keyboard shortcuts",
  group: "learn",
  order: 20,
  // The Settings open on the page they were left on: this one.
  run: () =>
    setStored("ui.settings_section", "Keyboard shortcuts")
      .catch(() => {})
      .then(() => workbench().openSettings()),
});

registerAction({ id: "help.whats_new", scope: "help", title: "What's new", group: "learn", order: 30, run: () => openWhatsNew() });

registerAction({ id: "help.report_bug", scope: "help", title: "Report a bug…", group: "report", order: 40, run: () => openExternal(BUG_REPORTS_URL) });
