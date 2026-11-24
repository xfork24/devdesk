import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { AppRoot } from "./DomainWorkbench";
import "./style.css";
import "./browser-compat";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <AppRoot />
  </StrictMode>,
);
