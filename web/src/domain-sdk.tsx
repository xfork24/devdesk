import { createContext, useContext } from "react";
export { createElement, useEffect, useState } from "react";

export type Data = Record<string, unknown>;

export type DomainContext = {
  query: (handler: string, input: Data) => Promise<unknown>;
};

export const DomainProvider = createContext<DomainContext | undefined>(
  undefined,
);

export function useDomain() {
  const context = useContext(DomainProvider);
  if (!context) throw new Error("领域页面必须由工作台挂载");
  return context;
}
