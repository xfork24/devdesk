import { AppRoot } from "../../../web/src/App";
import { useDomain } from "../../../web/src/domain-sdk";

export function BranchQueue() {
  return <AppRoot domain={useDomain()} initialTab="overview" />;
}

export function DeliveryWorkbench() {
  return <AppRoot domain={useDomain()} initialTab="execution" />;
}

export function EngineeringSettings() {
  return <AppRoot domain={useDomain()} initialTab="settings" />;
}
