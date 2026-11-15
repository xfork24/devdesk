import type { Request } from "./configuration-ui";
import { PermissionResources } from "./PermissionResources";

type Props = {
  request: Request;
  project?: { id: string };
  refresh: () => Promise<void>;
};

export function PermissionSettings({ request, project, refresh }: Props) {
  return (
    <PermissionResources
      request={request}
      refresh={refresh}
      projectId={project?.id}
    />
  );
}
