import { displayText } from "../sdk/display-text";
import { ActionButton } from "../sdk/ActionFeedback";
import type {
  PermissionCatalog,
  PermissionProfile,
} from "./permission-resources-state";

type Props = {
  catalog: PermissionCatalog;
  profileId?: string;
  onSelect?: (profile: PermissionProfile) => void;
  disabled?: boolean;
};

export function PermissionProfileHistory({
  catalog,
  profileId,
  onSelect,
  disabled,
}: Props) {
  const profile = catalog.profiles.find((p) => p.id === profileId);
  if (!profile) return null;
  const versions = catalog.profiles
    .filter((p) => p.identity === profile.identity)
    .sort((a, b) => b.version - a.version);
  return (
    <details>
      <summary>权限修订与来源历史</summary>
      {versions.map((version) => (
        <div key={version.id} className="permission-history-entry">
          {onSelect ? (
            <ActionButton
              type="button"
              disabled={disabled}
              onClick={() => onSelect(version)}
            >
              {displayText(version.name)} · v{version.version}
            </ActionButton>
          ) : (
            <strong>
              {displayText(version.name)} · v{version.version}
            </strong>
          )}
          <span>{displayText(version.id)}</span>
          <pre>
            {displayText(
              JSON.stringify(
                catalog.origins
                  .filter((origin) => origin.definition_id === version.id)
                  .map((origin) => origin.data),
                null,
                2,
              ),
            )}
          </pre>
        </div>
      ))}
    </details>
  );
}
