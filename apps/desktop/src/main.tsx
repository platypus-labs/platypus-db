import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { loadSettings } from "./settings";
import "./styles.css";
import { applyTheme, loadTheme } from "./theme";

applyTheme(loadTheme());

// Settings are a local file read: waiting for them avoids rendering defaults first.
void loadSettings().then(() =>
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  ),
);
