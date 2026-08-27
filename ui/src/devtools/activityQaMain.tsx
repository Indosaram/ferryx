import React from "react";
import ReactDOM from "react-dom/client";

import "../index.css";
import { ActivitySurfaceHarness } from "./ActivitySurfaceHarness";

const el = document.getElementById("root");
if (el) {
  ReactDOM.createRoot(el).render(
    <React.StrictMode>
      <ActivitySurfaceHarness />
    </React.StrictMode>,
  );
}
