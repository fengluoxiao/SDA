import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./styles.css";
import "./workbench.css";
import "./buttons.css";
import "./sliders.css";

try {
  document.documentElement.dataset.theme = localStorage.getItem("sda-theme") === "light" ? "light" : "dark";
} catch { /* Keep the default theme when storage is unavailable. */ }

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
