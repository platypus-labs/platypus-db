import { StrictMode } from "react";
// First: the Appearance settings are declared before the others, so the dialog lists them first.
import { paintCached, startAppearance } from "./theme";
import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { App } from "./App";
import { WindowApp } from "./shell/WindowApp";
import { translateAppMessages } from "./api";
import { loadSettings } from "./settings";
import { loadLanguage } from "./i18n";
import { appMessage } from "./storeErrors";
import { startModalA11y } from "./ui/modalA11y";
import { startErrorLog } from "./diagnostics/Diagnostics";
import { startAgentProposals } from "./agents/proposals";
import { startAgentBridge } from "./agents/bridge";
import "./styles/index.css";

paintCached();
const label = getCurrentWindow().label;
startErrorLog();
startModalA11y();
translateAppMessages(appMessage);

// Settings are a local file read: waiting for them avoids rendering defaults first.
void loadSettings()
  // Then the interface's language, when it is not one of the bundle.
  .then(() => loadLanguage().catch(console.error))
  .then(() => {
    startAppearance();
    startAgentProposals();
    startAgentBridge();
    createRoot(document.getElementById("root")!).render(
      <StrictMode>
        {/* The other windows show one tab: one moved there, or the quick query. */}
        {label === "main" ? <App /> : <WindowApp label={label} />}
      </StrictMode>,
    );
  });
