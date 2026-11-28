// The component kit entry: kernel-concept UI versioned with Web Core. A domain UI imports
// it as "@devdesk/web-kit", which the domain build redirects to /assets/devdesk-kit.mjs and
// keeps external, so the kit is not compiled into domain packages.
import { ConfigurationPage } from "../kit/ConfigurationPages";
import { ConfigurationLink } from "../kit/ConfigurationLink";
import { ConfigurationUsage } from "../kit/ConfigurationUsage";
import { Integrations } from "../kit/Integrations";
import { RuntimeSettings } from "../kit/RuntimeSettings";
import { TemplateSettings } from "../kit/TemplateSettings";
import { PermissionResources } from "../kit/PermissionResources";
import { PermissionProfileHistory } from "../kit/PermissionProfileHistory";
import { WorkflowScope } from "../kit/WorkflowScope";
import { WorkflowInstanceView } from "../kit/WorkflowInstanceView";
import { WorkflowProjectManager } from "../kit/WorkflowProjectManager";
import { RoleConversation } from "../kit/RoleConversation";
import { readingKeys, navigateReading } from "../kit/reading-state";
import { DshProviderSettings } from "../kit/DshProviderSettings";

export {
  ConfigurationPage,
  ConfigurationLink,
  ConfigurationUsage,
  Integrations,
  RuntimeSettings,
  TemplateSettings,
  PermissionResources,
  PermissionProfileHistory,
  WorkflowScope,
  WorkflowInstanceView,
  WorkflowProjectManager,
  RoleConversation,
  readingKeys,
  navigateReading,
  DshProviderSettings,
};
