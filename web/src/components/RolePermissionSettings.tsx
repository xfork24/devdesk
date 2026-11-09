import {
  ActionButton,
  ActionForm,
  ActionSelect,
  useActionFeedback,
} from "./ActionFeedback";
import { useEffect, useState } from "react";
export type PermissionRole = {
  id: string;
  name: string;
  version: number;
  body: string;
  agent_profile: Record<string, unknown>;
};
type Props = {
  roles: PermissionRole[];
  request: (method: string, params?: Record<string, unknown>) => Promise<any>;
  onSaved: () => Promise<void>;
};
export function RolePermissionSettings({ roles, request, onSaved }: Props) {
  const {
    message,
    setMessage,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [selected, setSelected] = useState("");
  const [value, setValue] = useState("{}");
  const [busy, setBusy] = useState(false);

  const role = roles.find((r) => r.id === selected) ?? roles[0];
  const current = JSON.stringify(
    role?.agent_profile.permission_overrides ?? {},
  );
  useEffect(() => {
    setValue(JSON.stringify(JSON.parse(current), null, 2));
  }, [role?.id, current]);
  return (
    <details>
      <summary>角色权限覆盖</summary>
      <label>
        角色
        <ActionSelect
          aria-label="权限角色"
          value={role?.id ?? ""}
          disabled={busy}
          onChange={(e) => {
            captureFeedback();
            return setSelected(e.target.value);
          }}
        >
          {roles.map((r) => (
            <option key={r.id} value={r.id}>
              {r.name} · v{r.version}
            </option>
          ))}
        </ActionSelect>
      </label>
      <label>
        角色覆盖 JSON
        <textarea
          aria-label="角色权限覆盖"
          rows={6}
          value={value}
          disabled={busy}
          onChange={(e) => {
            captureFeedback();
            return setValue(e.target.value);
          }}
        />
      </label>
      <p className="muted">
        角色覆盖优先于项目覆盖。保存会创建新的角色修订，已有会话继续使用原修订。
      </p>
      <ActionButton
        disabled={busy || !role}
        onClick={async () => {
          captureFeedback();

          if (!role) return;
          (beginFeedback(), setBusy(true));
          setMessage("");
          try {
            const result = await request("agent.revise", {
              command_id: crypto.randomUUID(),
              target: role.id,
              expected_revision: role.version,
              payload: {
                name: role.name,
                instructions: role.body,
                profile: {
                  ...role.agent_profile,
                  permission_overrides: JSON.parse(value),
                },
              },
            });
            await onSaved();
            setSelected(result.id);
            setMessage("已创建带权限覆盖的新角色修订。");
          } catch (error) {
            setMessage(String(error));
          } finally {
            (endFeedback(), setBusy(false));
          }
        }}
      >
        保存角色权限修订
      </ActionButton>
      <p role="status">{message}</p>
    </details>
  );
}
