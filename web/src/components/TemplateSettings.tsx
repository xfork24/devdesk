import { useEffect, useRef, useState } from "react";
import { ActionButton } from "./ActionFeedback";
import type { Request, Row } from "./configuration-ui";
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
    try {
      const result = await request(
        "configuration.templates.reset",
        command.current,
      );
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
          默认版本 {catalog?.bundle_version ?? "读取中…"}
          。编辑内容保存在数据库副本中，可恢复应用发布的完整默认定义。
        </p>
        {message && <p role="status">{message}</p>}
        {error && !preview && (
          <p role="alert" className="error">
            {error}
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
          {preview?.bundle_version}
          。自建定义、自定义修订和流程草稿将从当前目录归档。
        </p>
        <p>已有项目绑定、执行历史、会话、经验和登录凭据保留原值。</p>
        <p>
          自建或已修改定义 {preview?.custom_count ?? 0} 项；未发布流程草稿{" "}
          {preview?.draft_count ?? 0} 项；保留固定绑定的项目{" "}
          {preview?.pinned_projects?.length ?? 0} 个。
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
                {names[kind]}：当前 {preview?.current_counts?.[kind] ?? 0} 项 →
                默认 {preview?.restore_counts?.[kind] ?? 0} 项
              </li>
            ))}
        </ul>
        {error && (
          <p role="alert" className="error">
            {error}
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
          <ActionButton disabled={busy} onClick={openPreview}>
            重新预览
          </ActionButton>
          <ActionButton className="primary" disabled={busy} onClick={reset}>
            {busy ? "正在重置…" : "确认重置"}
          </ActionButton>
        </div>
      </dialog>
    </div>
  );
}
