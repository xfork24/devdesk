import { readGroupTab } from "./configuration-state";
import { deferPage } from "../sdk/deferred-page";

export const { Page: ResourceEditor, preload: preloadResourceEditor } =
  deferPage(() =>
    import("./AgentCatalog").then((module) => module.ResourceEditor),
  );
export const { Page: Integrations, preload: preloadIntegrations } = deferPage(
  () => import("./Integrations").then((module) => module.Integrations),
);
export const { Page: StandardManager, preload: preloadStandardManager } =
  deferPage(() =>
    import("./StandardManager").then((module) => module.StandardManager),
  );
export const { Page: WorkflowManager, preload: preloadWorkflowManager } =
  deferPage(() =>
    import("./WorkflowManager").then((module) => module.WorkflowManager),
  );
export const {
  Page: PermissionResources,
  preload: preloadPermissionResources,
} = deferPage(() =>
  import("./PermissionResources").then((module) => module.PermissionResources),
);

const processTabs = [
  ["workflow", "Workflow"],
  ["delivery_type", "Delivery"],
  ["spec", "Spec"],
  ["standard", "Standard"],
];
const foundationTabs = [
  ["model", "Model"],
  ["harness", "Harness"],
  ["sandbox", "Sandbox"],
  ["permission", "Permission"],
  ["mcp", "MCP"],
  ["skills", "Skills"],
  ["http", "Tools"],
  ["native", "Harness Tools"],
  ["internal", "Internal Tools"],
];

export const configurationTabs = {
  processes: processTabs,
  foundations: foundationTabs,
};
export type ConfigurationGroup = keyof typeof configurationTabs;

export function selectedConfigurationTab(group: ConfigurationGroup) {
  return readGroupTab(
    group,
    configurationTabs[group].map(([id]) => id),
  );
}

export function preloadConfigurationTab(group: ConfigurationGroup) {
  const tab = selectedConfigurationTab(group);
  const preload =
    tab === "workflow"
      ? preloadWorkflowManager
      : tab === "standard"
        ? preloadStandardManager
        : tab === "permission"
          ? preloadPermissionResources
          : ["model", "harness", "sandbox", "spec", "delivery_type"].includes(
                tab,
              )
            ? preloadResourceEditor
            : preloadIntegrations;
  void preload().catch(() => {});
}
