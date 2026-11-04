// Real-browser QA for the Supervisor panel (no daemon): the showcase renders every panel
// component and state from the shared fixtures, with inert writes.
//   ?theme=dark|light (default dark)  &touch=1 forces the phone scale on every block (390 px).
import React from "react";
import ReactDOM from "react-dom/client";

import "../index.css";
import "../settings-runtime.css";
import { SupervisorPanelShowcase } from "../components/supervisorPanel/SupervisorPanelShowcase";

const params = new URLSearchParams(window.location.search);
const theme = params.get("theme") === "light" ? "light" : "dark";
document.documentElement.classList.remove(theme === "light" ? "dark" : "light");
document.documentElement.classList.add(theme);
document.documentElement.dataset.theme = theme;
const touch = params.get("touch") === "1";

const rootEl = document.getElementById("root");
if (rootEl) {
  ReactDOM.createRoot(rootEl).render(
    <React.StrictMode>
      <div className="min-h-screen w-full bg-background">
        <SupervisorPanelShowcase touch={touch} />
      </div>
    </React.StrictMode>,
  );
  requestAnimationFrame(() => {
    document.documentElement.dataset.qaReady = "true";
  });
}
