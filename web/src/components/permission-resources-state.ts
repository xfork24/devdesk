import { useCallback, useEffect, useRef, useState } from "react";
import type { Request, Row } from "./rpc-types";

export type PermissionKind = "harness" | "sandbox";
export type PermissionProfile = {
  schema_version: number;
  scope: string;
  id: string;
  identity: string;
  name: string;
  version: number;
  target_kind: PermissionKind;
  policy: Record<string, unknown>;
};
export type PermissionTargetRef = {
  kind: PermissionKind;
  revision_id: string;
  content_hash: string;
};
export type PermissionTarget = {
  target: PermissionTargetRef;
  name: string;
  version: number;
  provider: string;
  runtime_version: string;
  supported: boolean;
  allowed_permission_ids?: string[];
  unsupported_reason?: string;
  platforms: string[];
  recipe_digest?: string;
  enforcement_contract?: string;
  registered_installations: unknown[];
  materializations: unknown[];
  active?: { revision: number; binding_revision_id: string };
};
export type PermissionBinding = {
  id: string;
  version: number;
  data: {
    target: PermissionTargetRef;
    permission_revision_id: string;
    enforcement_contract: string;
  };
};
export type PermissionOrigin = {
  id: string;
  definition_id: string;
  data: Record<string, unknown>;
};
export type PermissionCatalog = {
  profiles: PermissionProfile[];
  targets: PermissionTarget[];
  bindings: PermissionBinding[];
  origins: PermissionOrigin[];
  source: {
    path: string;
    builtin_path: string;
    is_builtin: boolean;
    revision: number;
  };
  default_sandbox_ref?: string;
};
export const profileView = (source: Row): PermissionProfile => ({
  ...source.data,
  scope: source.scope,
  identity: source.identity,
  version: source.revision,
  name: source.name,
  id: `${source.identity}:${source.revision}`,
});
export const profileSource = (p: PermissionProfile) => ({
  type: "permission_profile",
  identity: p.identity,
  revision: p.version,
  name: p.name,
  scope: p.scope,
  data: { schema_version: 2, target_kind: p.target_kind, policy: p.policy },
});

export function usePermissionResources(
  request: Request,
  projectId?: string,
  targetRef?: string,
  profileRef?: string,
) {
  const [catalog, setCatalog] = useState<PermissionCatalog>();
  const [error, setError] = useState("");
  const epoch = useRef(0);
  const reload = useCallback(async () => {
    const ticket = ++epoch.current;
    try {
      const value = await request("query.permission_resources", {
        project_id: projectId,
        target_ref: targetRef,
        profile_ref: profileRef,
      });
      if (ticket === epoch.current) {
        setCatalog({ ...value, profiles: value.profiles.map(profileView) });
        setError("");
      }
    } catch (e) {
      if (ticket === epoch.current) setError(String(e));
      throw e;
    }
  }, [request, projectId, targetRef, profileRef]);
  useEffect(() => {
    setCatalog(undefined);
    setError("");
  }, [projectId, targetRef, profileRef]);
  useEffect(() => {
    void reload().catch(() => {});
    return () => {
      epoch.current += 1;
    };
  }, [reload]);
  return { catalog, error, reload };
}
