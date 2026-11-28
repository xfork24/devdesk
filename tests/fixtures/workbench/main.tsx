import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { AppRoot } from "../../../web/src/core/App";
import "../../../web/src/core/style.css";
import "../../../web/src/core/browser-compat";
import "../../../plugins/software-development/ui/web/workbench.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <AppRoot />
  </StrictMode>,
);
