import type { Row } from "../sdk/rpc-types";

export function configurationHref(
  record: Row,
  projectId?: string,
  position?: { node?: string; port?: string },
) {
  if (!record.id || !record.type) return undefined;
  const params = new URLSearchParams();
  const view = new URLSearchParams(location.search).get("view");
  if (view) params.set("view", view);
  const type = record.type;
  if (["role", "capability", "agent"].includes(type)) {
    params.set("tab", "actors");
    params.set("config_tab", type);
    params.set("config_object", record.id);
  } else if (["spec", "delivery_type", "standard", "workflow"].includes(type)) {
    params.set("tab", "workflows");
    params.set("config_tab", type);
    params.set(
      type === "workflow"
        ? "workflow_id"
        : type === "standard"
          ? "standard_id"
          : "config_object",
      record.id,
    );
    if (type === "workflow" && position?.node)
      params.set("workflow_node", position.node);
    if (type === "workflow" && position?.port)
      params.set("workflow_port", position.port);
  } else if (
    [
      "skill",
      "model",
      "harness",
      "sandbox",
      "permission_profile",
      "tool",
    ].includes(type)
  ) {
    params.set("tab", "components");
    params.set(
      "config_tab",
      type === "skill"
        ? "skills"
        : type === "permission_profile"
          ? "permission"
          : type,
    );
    params.set(
      type === "permission_profile" ? "permission_ref" : "config_object",
      record.id,
    );
    if (type === "tool") {
      const implementation = record.data?.implementation;
      const tab =
        implementation === "harness.native"
          ? "native"
          : implementation === "platform"
            ? "internal"
            : implementation?.kind === "mcp"
              ? "mcp"
              : "http";
      params.set("config_tab", tab);
      if (tab === "http" || tab === "mcp") {
        params.set("tool_ref", record.id);
        if (record.data?.integration_ref)
          params.set("config_object", record.data.integration_ref);
        else params.delete("config_object");
      }
    }
  } else return undefined;
  if (projectId) params.set("project_id", projectId);
  return `?${params}`;
}
