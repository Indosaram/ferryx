import React, { useEffect } from "react";
import { createRoot } from "react-dom/client";

import { useApplyAppearanceSettings } from "../lib/appearanceSettings";
import { AppearanceQaFixture, seedAppearanceQaState } from "./chatQaStates";
import "../index.css";
import "../settings-runtime.css";

const params = new URLSearchParams(window.location.search);
const state = params.get("state") ?? "appearance-preview";
const theme = params.get("theme") === "light" ? "light" : "dark";
seedAppearanceQaState(state, theme);

function Harness() {
  useApplyAppearanceSettings();
  useEffect(() => {
    document.documentElement.dataset.qaReady = "true";
    return () => { delete document.documentElement.dataset.qaReady; };
  }, []);
  return <AppearanceQaFixture state={state} />;
}

const root = document.getElementById("root");
if (root) createRoot(root).render(<React.StrictMode><Harness /></React.StrictMode>);
