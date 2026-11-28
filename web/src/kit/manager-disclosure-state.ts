import { createContext } from "react";

export const ManagerDisclosureContext = createContext<{
  revision: number;
  reset: () => void;
} | null>(null);
