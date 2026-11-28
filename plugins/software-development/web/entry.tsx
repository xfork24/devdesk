import { AppRoot } from "../../../web/src/core/App";
import { useDomain } from "../../../web/src/sdk/domain-sdk";

export function BranchQueue() {
  return <AppRoot domain={useDomain()} initialTab="overview" />;
}

export function DeliveryWorkbench() {
  return <AppRoot domain={useDomain()} initialTab="execution" />;
}

export function EngineeringSettings() {
  return <AppRoot domain={useDomain()} initialTab="settings" />;
}
