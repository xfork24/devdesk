import { PermissionResources } from "./PermissionResources";

type Props = {
  request: (method: string, params?: Record<string, unknown>) => Promise<any>;
  project?: { id: string };
  refresh: () => Promise<void>;
};

export function PermissionSettings({ request, project, refresh }: Props) {
  return <PermissionResources request={request} refresh={refresh} projectId={project?.id} />;
}
