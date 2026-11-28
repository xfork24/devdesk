import type { Row } from "../sdk/rpc-types";

export function experienceKey(entry: Row) {
  return `${entry.project_id}:${entry.snapshot_id}`;
}
