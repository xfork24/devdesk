import { createContext, useContext } from "react";
import type { Request } from "./components/rpc-types";

export {
  Children,
  Component,
  Fragment,
  Suspense,
  cloneElement,
  createContext,
  createElement,
  forwardRef,
  isValidElement,
  lazy,
  memo,
  startTransition,
  useCallback,
  useContext,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
export { default } from "react";
export { createPortal } from "react-dom";
export { jsx, jsxs } from "react/jsx-runtime";
export { DomainLayout, WorkbenchLayout } from "./workbench-layout";
export {
  ActionButton,
  ActionForm,
  ActionInput,
  ActionSelect,
} from "./components/ActionFeedback";
export * from "./components/action-feedback";

export type Data = Record<string, unknown>;

export type DomainContext = {
  epoch: number;
  view: string;
  projectConnection?: string;
  query: (handler: string, input: Data) => Promise<unknown>;
  request: Request;
  events: (projectId?: string) => EventSource;
};

export const DomainProvider = createContext<DomainContext | undefined>(
  undefined,
);

export function useDomain() {
  const context = useContext(DomainProvider);
  if (!context) throw new Error("领域页面必须由工作台挂载");
  return context;
}
