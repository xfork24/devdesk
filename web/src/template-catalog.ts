import { useSyncExternalStore } from "react";

let generation: number | undefined;
const listeners = new Set<() => void>();
export const getCatalogGeneration = () => generation;
export function observeCatalogGeneration(value: unknown) {
  if (
    typeof value !== "number" ||
    (generation !== undefined && value <= generation)
  )
    return;
  generation = value;
  listeners.forEach((listener) => listener());
}
export function useCatalogGeneration() {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    () => generation ?? 1,
  );
}
export function isTemplateMutation(method: string) {
  const [kind, action] = method.split(".");
  return (
    ([
      "role",
      "skill",
      "spec",
      "standard",
      "workflow",
      "delivery_type",
      "capability",
      "agent",
      "model",
      "harness",
      "sandbox",
      "tool",
      "integration",
    ].includes(kind) &&
      ["create", "revise", "delete", "retire", "disable"].includes(action)) ||
    [
      "configuration.retire",
      "workflow.publish",
      "workflow.draft.create",
      "workflow.draft.update",
      "workflow.draft.upgrade",
      "skill.import",
      "skill.import_package",
      "integration.openapi.import",
      "integration.tools.publish",
      "integration.binding.create",
      "integration.binding.revise",
      "integration.tool.create",
      "integration.tool.revise",
      "permission.profile.create",
      "permission.profile.revise",
      "permission.profile.import",
      "permission.profile.reload",
    ].includes(method)
  );
}
