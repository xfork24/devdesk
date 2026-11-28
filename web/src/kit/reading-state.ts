import { useEffect, useState } from "react";
import type { Row, Request } from "../sdk/rpc-types";
export const readingKeys = [
  "instance_id",
  "execution_id",
  "output_id",
  "agent_instance_id",
  "session_id",
  "turn_id",
] as const;
export function executionUrl(
  detail: string,
  projectId: string,
  ownerId: string,
  target: Row = {},
) {
  const p = new URLSearchParams(location.search);
  for (const key of readingKeys) p.delete(key);
  p.set("tab", "execution");
  p.set("detail", detail);
  p.set("project_id", projectId);
  if (
    new URLSearchParams(location.search).get("project_id") !== projectId ||
    target.feature_id ||
    target.task_id
  ) {
    p.delete("feature_id");
    p.delete("task_id");
  }
  if (
    !target.task_id &&
    !target.feature_id &&
    ownerId !== projectId &&
    !p.get("feature_id")
  )
    p.set("feature_id", ownerId);
  for (const [key, value] of Object.entries(target))
    if (value) p.set(key, String(value));
  return `?${p}`;
}
export function navigateReading(url: string) {
  history.pushState(null, "", url);
  window.dispatchEvent(new PopStateEvent("popstate"));
}
export function useReading(
  request: Request,
  method: string,
  params: Row,
  enabled = true,
  revision?: number,
) {
  const key = JSON.stringify([method, params, revision]);
  const [attempt, setAttempt] = useState(0);
  const [state, setState] = useState<{
    key: string;
    value?: Row;
    error?: string;
    loading: boolean;
  }>({ key: "", loading: false });
  useEffect(() => {
    if (!enabled) return;
    let active = true;
    const controller = new AbortController();
    setState({ key, loading: true });
    void request(method, JSON.parse(key)[1], { signal: controller.signal })
      .then((value) => {
        if (active) setState({ key, value, loading: false });
      })
      .catch((error) => {
        if (active) setState({ key, error: String(error), loading: false });
      });
    return () => {
      active = false;
      controller.abort();
    };
  }, [request, method, key, enabled, attempt]);
  const current =
    state.key === key && enabled
      ? state
      : { key, value: undefined, error: undefined, loading: enabled };
  return { ...current, retry: () => setAttempt((n) => n + 1) };
}
const names: Record<string, string> = {
  actor: "执行身份",
  kind: "类型",
  operation: "操作",
  driver_id: "调度器",
  controller_id: "控制器",
  override_completion: "人工例外批准",
  value: "内容",
  delivery_ref: "交付类型",
  exitCode: "退出码",
  agent_instance_id: "Agent 实例",
  session_id: "会话",
  turn_id: "轮次",
  execution_id: "步骤执行",
  plan_revision_id: "冻结计划",
  role_ref: "职责引用",
  summary: "摘要",
  description: "说明",
  instructions: "任务指令",
  title: "标题",
  status: "状态",
  reason: "原因",
  error: "错误",
  message: "消息",
  result: "结果",
  response: "响应",
  request: "请求",
  arguments: "请求参数",
  output: "输出",
  text: "正文",
  content: "内容",
  approve: "批准结论",
  checks: "检查项",
  passed: "通过",
  evidence: "证据",
  tasks: "任务",
  dependencies: "依赖",
  command: "命令",
  exit_code: "退出码",
  stdout: "标准输出",
  stderr: "错误输出",
  failure_type: "失败分类",
  candidate: "被测版本",
  environment: "环境",
  expected_failure: "预期失败",
  execution: "执行",
  inputs: "输入",
  outputs: "输出要求",
  input_sources: "输入来源",
  completion: "完成条件",
  authorization: "授权",
  runtime_context: "运行上下文",
};
export const fieldName = (key: string) => names[key] || key;
