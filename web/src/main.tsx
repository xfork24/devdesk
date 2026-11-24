import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { AppRoot } from "./App";
import "./style.css";
import "./browser-compat";
import "../../configurations/software-development/web/workbench.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <AppRoot />
  </StrictMode>,
);
