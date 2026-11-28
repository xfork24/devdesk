import { createContext } from "react";
import type { Request } from "../sdk/rpc-types";
export const ConfigurationScope = createContext<
  | { request: Request; projectId?: string; storageNamespace?: string }
  | undefined
>(undefined);
