import { useState } from "react";
import { ActionButton, ActionSelect } from "./ActionFeedback";
import { useActionFeedback } from "./action-feedback";
import { PermissionProfileHistory } from "./PermissionProfileHistory";
import {
  usePermissionResources,
  type PermissionKind,
} from "./permission-resources-state";
import type { Request } from "./rpc-types";

type Props = {
  request: Request;
  projectId?: string;
  targetRef: string;
  kind: PermissionKind;
};

export function PermissionTargetBinding(props: Props) {
  // Each immutable target revision owns its draft selection and preview results.
  return (
    <TargetBinding key={`${props.projectId}:${props.targetRef}`} {...props} />
  );
}

function TargetBinding({ request, projectId, targetRef, kind }: Props) {
  const { catalog, error, reload } = usePermissionResources(
    request,
    projectId,
    targetRef,
  );
  const { message, setMessage, capture, beginFeedback, endFeedback } =
    useActionFeedback();
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<string>();
  const [bindingPreview, setBindingPreview] = useState<unknown>();
  const [scopePreview, setScopePreview] = useState<unknown>();
  const [counterpart, setCounterpart] = useState("");
  const [scope, setScope] = useState(
    '{"work_type":"discussion","writable_paths":[],"read_only_paths":[]}',
  );
  const target = catalog?.targets.find(
    (t) => t.target.revision_id === targetRef && t.target.kind === kind,
  );
  const history = (catalog?.bindings || [])
    .filter(
      (b) =>
        b.data.target.revision_id === targetRef &&
        b.data.target.content_hash === target?.target.content_hash,
    )
    .sort((a, b) => b.version - a.version);
  const active = history.find(
    (b) => b.id === target?.active?.binding_revision_id,
  );
  const value = selected ?? active?.data.permission_revision_id ?? "";
  const profiles = (catalog?.profiles || []).filter(
    (p) =>
      p.target_kind === kind &&
      (!target?.allowed_permission_ids ||
        target.allowed_permission_ids.includes(p.id)),
  );
  const profile = profiles.find((p) => p.id === value);
  const otherKind = kind === "harness" ? "sandbox" : "harness";
  const candidates = (catalog?.targets || []).filter(
    (t) => t.target.kind === otherKind,
  );
  const otherRef =
    counterpart ||
    (otherKind === "sandbox" ? catalog?.default_sandbox_ref : "") ||
    "";

  async function action(work: () => Promise<void>) {
    capture();
    beginFeedback();
    setBusy(true);
    setMessage("");
    try {
      await work();
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
      endFeedback();
    }
  }
  if (!catalog)
    return (
      <p role={error ? "alert" : "status"}>{error || "正在读取目标权限…"}</p>
    );
  if (!target) return <p role="alert">此目标修订没有可用的权限执行合同。</p>;
  return (
    <section
      className="permission-target-binding"
      aria-label={`${kind === "harness" ? "Harness" : "Sandbox"} 权限绑定`}
    >
      <h3>目标版本绑定</h3>
      <p>
        当前目标：{target.name} · v{target.version} · {targetRef}
      </p>
      <p>
        当前绑定：{target.active?.binding_revision_id || "尚未分配"} · revision{" "}
        {target.active?.revision || 0}
      </p>
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
      {!target.supported && (
        <p className="error">{target.unsupported_reason}</p>
      )}
      <fieldset disabled={busy}>
        <legend>选择目标权限</legend>
        <label>
          应用到目标的权限
          <ActionSelect
            aria-label="目标资源权限"
            value={value}
            onChange={(e) => {
              setSelected(e.target.value);
              setBindingPreview(undefined);
            }}
          >
            <option value="">
              {profiles.length
                ? "请选择可应用的权限修订"
                : "此目标没有兼容的权限修订"}
            </option>
            {profiles.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name} · v{p.version} · {p.target_kind}
              </option>
            ))}
          </ActionSelect>
        </label>
        <div className="actions">
          <ActionButton
            type="button"
            disabled={!profile || !target.supported}
            onClick={() =>
              action(async () => {
                setBindingPreview(
                  await request("permission.binding.preflight", {
                    project_id: projectId,
                    target: target.target,
                    permission_revision_id: value,
                  }),
                );
                setMessage("目标兼容性预检通过，仍需实际执行范围校验。");
              })
            }
          >
            预检目标绑定
          </ActionButton>
          <ActionButton
            type="button"
            disabled={!profile || !target.supported}
            onClick={() =>
              action(async () => {
                await request("permission.bindings.apply", {
                  project_id: projectId,
                  command_id: crypto.randomUUID(),
                  payload: {
                    bindings: [
                      {
                        target: target.target,
                        permission_revision_id: value,
                        expected_revision: target.active?.revision || 0,
                      },
                    ],
                  },
                });
                setScopePreview(undefined);
                setBindingPreview(undefined);
                await reload();
                setMessage(
                  "活动绑定已更新；已有工作保留冻结权限，追随目标的讨论将在下一轮切换。",
                );
              })
            }
          >
            应用到该目标版本
          </ActionButton>
          <a
            href={`?tab=components&config_tab=permission&project_id=${encodeURIComponent(projectId || "")}&permission_ref=${encodeURIComponent(value)}`}
          >
            查看 Permission 定义与来源
          </a>
        </div>
      </fieldset>
      {bindingPreview !== undefined && (
        <details open>
          <summary>目标兼容性预检结果</summary>
          <pre>{JSON.stringify(bindingPreview, null, 2)}</pre>
        </details>
      )}
      <details>
        <summary>构建、平台与安装登记</summary>
        <p className="muted">
          安装登记仍需在执行前核验。目标兼容性预检不会下载或安装运行环境。
        </p>
        <pre>
          {JSON.stringify(
            {
              target: target.target,
              platforms: target.platforms,
              recipe_digest: target.recipe_digest,
              enforcement_contract: target.enforcement_contract,
              registered_installations: target.registered_installations,
              materializations: target.materializations,
            },
            null,
            2,
          )}
        </pre>
      </details>
      <details>
        <summary>实际执行范围预览</summary>
        <p className="muted">
          以当前已应用的双层绑定预览；选择尚未应用的权限不会改变此结果。
        </p>
        <fieldset disabled={busy}>
          <legend>双层执行范围</legend>
          <label>
            {otherKind === "harness" ? "Harness 组合目标" : "Sandbox 组合目标"}
            <ActionSelect
              aria-label={
                otherKind === "harness"
                  ? "Harness 组合目标"
                  : "Sandbox 组合目标"
              }
              value={otherRef}
              onChange={(e) => {
                setCounterpart(e.target.value);
                setScopePreview(undefined);
              }}
            >
              <option value="">
                选择{otherKind === "harness" ? " Harness" : " Sandbox"}修订
              </option>
              {candidates.map((t) => (
                <option key={t.target.revision_id} value={t.target.revision_id}>
                  {t.name} · v{t.version}
                </option>
              ))}
            </ActionSelect>
          </label>
          <label>
            ExecutionScope JSON
            <textarea
              aria-label="ExecutionScope JSON"
              rows={5}
              value={scope}
              onChange={(e) => {
                setScope(e.target.value);
                setScopePreview(undefined);
              }}
            />
          </label>
          <ActionButton
            type="button"
            disabled={busy || !otherRef || !active || !target.supported}
            onClick={() =>
              action(async () => {
                setScopePreview(
                  await request("permission.binding.preflight", {
                    project_id: projectId,
                    harness_ref: kind === "harness" ? targetRef : otherRef,
                    selection: {
                      sandbox_ref: kind === "sandbox" ? targetRef : otherRef,
                      harness: { mode: "follow_target" },
                      sandbox: { mode: "follow_target" },
                    },
                    scope: JSON.parse(scope),
                  }),
                );
                setMessage("完整范围预览通过；正式执行仍由协调器核验并冻结。");
              })
            }
          >
            预览两层实际权限
          </ActionButton>
        </fieldset>
        {scopePreview !== undefined && (
          <details open>
            <summary>权限解析结果</summary>
            <pre>{JSON.stringify(scopePreview, null, 2)}</pre>
          </details>
        )}
      </details>
      <details>
        <summary>目标绑定历史</summary>
        {history.length ? (
          history.map((binding) => (
            <div className="permission-history-entry" key={binding.id}>
              <strong>
                绑定 v{binding.version}
                {binding.id === active?.id ? " · 当前" : ""}
              </strong>
              <span>{binding.data.permission_revision_id}</span>
              <ActionButton
                type="button"
                disabled={
                  busy ||
                  !profiles.some(
                    (p) => p.id === binding.data.permission_revision_id,
                  )
                }
                onClick={() => {
                  setSelected(binding.data.permission_revision_id);
                  setBindingPreview(undefined);
                }}
              >
                选择此历史权限
              </ActionButton>
              <a
                href={`?tab=components&config_tab=permission&permission_ref=${encodeURIComponent(binding.data.permission_revision_id)}&project_id=${encodeURIComponent(projectId || "")}`}
              >
                查看权限来源
              </a>
            </div>
          ))
        ) : (
          <p className="muted">此目标修订尚无绑定历史。</p>
        )}
      </details>
      <PermissionProfileHistory
        catalog={catalog}
        profileId={active?.data.permission_revision_id}
      />
      {message && <p role="status">{message}</p>}
    </section>
  );
}
