import { createContext } from "react";
import type { Request } from "./rpc-types";
export const ConfigurationScope = createContext<
  { request: Request; projectId?: string } | undefined
>(undefined);
