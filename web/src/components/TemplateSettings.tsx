import { displayText } from "../display-text";
import { useEffect, useRef, useState } from "react";
import { ActionButton } from "./ActionFeedback";
import { type Request, type Row } from "./rpc-types";
import { observeCatalogGeneration } from "../template-catalog";

const names: Record<string, string> = {
  role: "Role",
  skill: "Skill",
  spec: "Spec",
  standard: "Standard",
  workflow: "Workflow",
  workflow_draft: "流程草稿",
  delivery_type: "Delivery",
  capability: "Capability",
  agent: "Agent",
  model: "Model",
  harness: "Harness",
  sandbox: "Sandbox",
  tool: "Tools",
  integration: "MCP / HTTP 接入",
  integration_binding: "接入绑定",
  permission_profile: "Permission",
};
export function TemplateSettings({
  request,
  refresh,
}: {
  request: Request;
  refresh: () => Promise<void>;
}) {
  const [catalog, setCatalog] = useState<Row>();
  const [preview, setPreview] = useState<Row>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const dialog = useRef<HTMLDialogElement>(null);
  const command = useRef<{ command_id: string; payload: Row } | undefined>(
    undefined,
  );
  useEffect(() => {
    let active = true;
    void request("query.template_catalog")
      .then((value) => {
        if (active) setCatalog(value);
      })
      .catch((e) => {
        if (active) setError(String(e));
      });
    return () => {
      active = false;
    };
  }, [request]);
  async function openPreview() {
    setBusy(true);
    setError("");
    setMessage("");
    try {
      const value = await request("query.template_reset_preview");
      setPreview(value);
      command.current = undefined;
      if (!dialog.current?.open) dialog.current?.showModal();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function updateFactory() {
    if (busy) return;
    dialog.current?.close();
    setPreview(undefined);
    setBusy(true);
    setError("");
    setMessage("");
    try {
      const basis = await request("query.template_reset_preview");
      const result = await request("configuration.templates.reset", {
        command_id: crypto.randomUUID(),
        payload: {
          expected_catalog_generation: basis.generation,
          catalog_fingerprint: basis.catalog_fingerprint,
          bundle_hash: basis.bundle_hash,
          preserve_custom: true,
        },
      });
      observeCatalogGeneration(result.generation);
      dialog.current?.close();
      setPreview(undefined);
      command.current = undefined;
      setMessage(
        result.changed
          ? "默认配置已恢复；自定义定义和已保存项目选择保留。"
          : "已是当前默认配置；自定义定义保留。",
      );
      setCatalog(await request("query.template_catalog"));
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function reset() {
    if (!preview || busy) return;
    setBusy(true);
    setError("");
    command.current ??= {
      command_id: crypto.randomUUID(),
      payload: {
        expected_catalog_generation: preview.generation,
        catalog_fingerprint: preview.catalog_fingerprint,
        bundle_hash: preview.bundle_hash,
      },
    };
    dialog.current?.close();
    setPreview(undefined);
    try {
      const result = await request(
        "configuration.templates.reset",
        command.current,
      );
      const params = new URLSearchParams(location.search);
      params.delete("standard_id");
      params.delete("standard_history");
      history.replaceState(null, "", `?${params}`);
      observeCatalogGeneration(result.generation);
      dialog.current?.close();
      setPreview(undefined);
      command.current = undefined;
      setMessage(
        result.changed
          ? `模板定义已恢复默认，共 ${Object.values(result.counts as Record<string, number>).reduce((a, b) => a + b, 0)} 项。`
          : "模板定义已是当前默认版本。",
      );
      setCatalog(await request("query.template_catalog"));
      try {
        await refresh();
      } catch {
        setError("重置已提交，页面刷新失败，请重新载入。");
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="setting-row">
      <div>
        <h3>模板定义</h3>
        <p className="muted">
          默认版本 {displayText(catalog?.bundle_version ?? "读取中…")}
          。编辑内容保存在数据库副本中，可恢复应用发布的完整默认定义。
        </p>
        <p className="muted">
          可在弹窗中选择保留自定义定义并恢复默认配置，或完全重置配置。
        </p>
        {message && <p role="status">{displayText(message)}</p>}
        {error && !preview && (
          <p role="alert" className="error">
            {displayText(error)}
          </p>
        )}
      </div>
      <ActionButton disabled={busy} onClick={openPreview}>
        重置模板定义
      </ActionButton>
      <dialog
        ref={dialog}
        className="template-reset-dialog"
        aria-labelledby="template-reset-title"
        onCancel={(event) => {
          if (busy) event.preventDefault();
          else {
            setPreview(undefined);
            setError("");
          }
        }}
      >
        <h2 id="template-reset-title">确认重置模板定义</h2>
        <p>
          将当前数据集的全局与项目模板定义恢复为默认版本{" "}
          {displayText(preview?.bundle_version)}
          。自建定义、自定义修订、流程草稿和配置历史将被删除，失效的项目配置绑定将清除。
        </p>
        <p>
          项目、仓库、执行记录、会话、经验和登录凭据保留；受影响的项目需要重新配置。
        </p>
        <p>
          “恢复默认配置”更新默认配置并保留自定义定义；“重置配置”完全恢复为预定义配置。
        </p>
        <p>
          自建或已修改定义 {displayText(preview?.custom_count ?? 0)}{" "}
          项；未发布流程草稿 {displayText(preview?.draft_count ?? 0)}{" "}
          项；已有配置的项目{" "}
          {displayText(preview?.pinned_projects?.length ?? 0)} 个。
        </p>
        <ul>
          {[
            ...new Set([
              ...Object.keys(preview?.current_counts ?? {}),
              ...Object.keys(preview?.restore_counts ?? {}),
            ]),
          ]
            .filter((kind) => names[kind])
            .map((kind) => (
              <li key={kind}>
                {displayText(names[kind])}：当前{" "}
                {displayText(preview?.current_counts?.[kind] ?? 0)} 项 → 默认{" "}
                {displayText(preview?.restore_counts?.[kind] ?? 0)} 项
              </li>
            ))}
        </ul>
        {error && (
          <p role="alert" className="error">
            {displayText(error)}
          </p>
        )}
        <div className="actions">
          <ActionButton
            disabled={busy}
            onClick={() => {
              dialog.current?.close();
              setPreview(undefined);
              setError("");
            }}
          >
            取消
          </ActionButton>
          <ActionButton disabled={busy} onClick={updateFactory}>
            恢复默认配置
          </ActionButton>
          <ActionButton className="primary" disabled={busy} onClick={reset}>
            {displayText(busy ? "正在重置…" : "重置配置")}
          </ActionButton>
        </div>
      </dialog>
    </div>
  );
}
