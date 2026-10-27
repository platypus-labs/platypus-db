import { StrictMode } from "react";
// First: the Appearance settings are declared before the others, so the dialog lists them first.
import { paintCached, startAppearance } from "./theme";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { translateAppMessages } from "./api";
import { loadSettings } from "./settings";
import { appMessage } from "./storeErrors";
import { startModalA11y } from "./ui/modalA11y";
import "./styles/index.css";

paintCached();
startModalA11y();
translateAppMessages(appMessage);

// Settings are a local file read: waiting for them avoids rendering defaults first.
void loadSettings().then(() => {
  startAppearance();
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
});
