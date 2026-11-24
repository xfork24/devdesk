import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { AppRoot } from "../../src/App";
import "../../src/style.css";
import "../../src/browser-compat";
import "../../../configurations/software-development/web/workbench.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <AppRoot />
  </StrictMode>,
);
