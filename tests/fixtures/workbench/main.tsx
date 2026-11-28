import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { AppRoot } from "../../../web/src/App";
import "../../../web/src/style.css";
import "../../../web/src/browser-compat";
import "../../../plugins/software-development/ui/web/workbench.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <AppRoot />
  </StrictMode>,
);
