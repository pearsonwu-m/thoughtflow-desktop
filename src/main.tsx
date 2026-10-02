import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { currentWindowLabel } from "./lib/api";
import { SettingsApp } from "./settings/SettingsApp";
import "./styles/tokens.css";
import "./styles/widget.css";
import "./styles/settings.css";
import { WidgetApp } from "./widget/WidgetApp";

// One bundle serves both windows; the window label picks the root.
void currentWindowLabel().then((label) => {
  document.documentElement.dataset.window = label;
  const root = document.getElementById("root");
  if (!root) return;
  createRoot(root).render(<StrictMode>{label === "settings" ? <SettingsApp /> : <WidgetApp />}</StrictMode>);
});
