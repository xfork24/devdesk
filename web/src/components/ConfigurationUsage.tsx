import { useEffect, useState, useSyncExternalStore } from "react";
import { displayText } from "../display-text";
import {
  getConfigurationQueryEpoch,
  subscribeConfigurationQueries,
} from "../configuration-client";
import { ActionButton } from "./ActionFeedback";
import { compareConfigurationRecords } from "./configuration-state";
import type { Request } from "./rpc-types";
import { useReadRequest } from "./use-read-request";
import { configurationHref } from "./configuration-links";
import { ConfigurationLink } from "./ConfigurationLink";

type Usage = {
  reference: string;
  name: string;
  type: string;
  version: number;
  scope: string;
  path: string;
  kind:
    | "reference"
    | "workflow_input"
    | "workflow_output"
    | "step_input"
    | "step_argument"
    | "step_collection"
    | "step_output"
    | "step_spec"
    | "step_tool";
  stage?: { key: string; name: string };
  step?: { key: string; name: string };
  port?: string;
};
type Props = {
  request: Request;
  projectId?: string;
  id?: string;
  kind: string;
};

function usageDescription(location: Usage) {
  return `${location.stage ? `阶段：${location.stage.name} · ` : ""}${location.step ? `步骤：${location.step.name} · ` : ""}${
    {
      workflow_input: "流程输入",
      workflow_output: "流程输出",
      step_input: "输入",
      step_output: "输出",
      step_argument: "工具参数",
      step_collection: "遍历来源",
      step_spec: "适用规范",
      step_tool: "执行工具",
      reference: "引用",
    }[location.kind]
  }${location.port ? `：${location.port}` : ""}`;
}

function usageLink(user: Usage, projectId?: string) {
  return configurationHref({ ...user, id: user.reference }, projectId);
}

export function ConfigurationUsage(props: Props) {
  const epoch = useSyncExternalStore(
    subscribeConfigurationQueries,
    getConfigurationQueryEpoch,
  );
  if (!props.id) return null;
  return (
    <UsageDetails
      key={`${props.projectId || "global"}:${props.id}:${epoch}`}
      {...props}
      id={props.id}
    />
  );
}

function UsageDetails({
  request: parentRequest,
  projectId,
  id,
  kind,
}: Props & { id: string }) {
  const request = useReadRequest(parentRequest);
  const [result, setResult] = useState<{
    users: Usage[];
    total: number;
    index_version: string;
  }>();
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    void request(
      "query.configuration_usage",
      { id, project_id: projectId, limit: 100 },
      { signal: controller.signal },
    )
      .then((value) => {
        if (!controller.signal.aborted) {
          setResult(value);
          setError("");
          setLoading(false);
        }
      })
      .catch((error) => {
        if (!controller.signal.aborted) {
          setError(String(error));
          setLoading(false);
        }
      });
    return () => controller.abort();
  }, [request, projectId, id, attempt]);
  const groups = new Map<string, { user: Usage; locations: Usage[] }>();
  for (const user of result?.users || []) {
    const group = groups.get(user.reference);
    if (group) group.locations.push(user);
    else groups.set(user.reference, { user, locations: [user] });
  }
  const users = [...groups.values()].sort((a, b) =>
    compareConfigurationRecords(
      { ...a.user, id: a.user.reference },
      { ...b.user, id: b.user.reference },
    ),
  );
  const label =
    kind === "spec"
      ? "Delivery / Capability / Workflow 引用"
      : kind === "delivery_type"
        ? "Workflow 引用位置"
        : kind === "tool"
          ? "Capability / Workflow 引用"
          : "Capability 引用";
  return (
    <aside
      className="configuration-usage full-width"
      aria-label="被引用情况"
      aria-busy={loading}
    >
      <h3>{label}</h3>
      {loading && !result ? <p className="muted">正在读取引用关系…</p> : null}
      {!loading && !error && !users.length ? (
        <p className="muted">当前没有配置引用此修订。</p>
      ) : null}
      {users.length ? (
        <ul>
          {users.map(({ user, locations }) => (
            <li key={user.reference}>
              <span>{displayText(user.name)}</span>
              <span className="muted">
                {" "}
                ·{" "}
                {user.type === "delivery_type"
                  ? "Delivery"
                  : user.type === "workflow"
                    ? "Workflow"
                    : "Capability"}{" "}
                · v{user.version}
              </span>
              <ConfigurationLink
                href={usageLink(user, projectId)}
                label={user.name}
              />
              {user.type === "workflow" ? (
                <ul>
                  {[...locations]
                    .sort((a, b) => {
                      const order = (location: Usage) =>
                        location.kind === "workflow_input"
                          ? -1
                          : location.kind === "workflow_output"
                            ? Number.MAX_SAFE_INTEGER
                            : Number(
                                location.path.match(/\/steps\/(\d+)/)?.[1] || 0,
                              );
                      return (
                        order(a) - order(b) || a.path.localeCompare(b.path)
                      );
                    })
                    .map((location) => (
                      <li key={location.path}>
                        <span>{usageDescription(location)}</span>
                        <ConfigurationLink
                          href={configurationHref(
                            { ...user, id: user.reference },
                            projectId,
                            {
                              node: location.step
                                ? `step:${location.step.key}`
                                : location.stage
                                  ? `stage:${location.stage.key}`
                                  : "workflow",
                              port:
                                location.kind === "step_input"
                                  ? `input:${location.port}`
                                  : location.kind === "step_output"
                                    ? `output:${location.port}`
                                    : location.kind === "step_argument"
                                      ? `argument:${location.port}`
                                      : location.kind === "step_collection"
                                        ? "collection"
                                        : location.kind === "workflow_input" ||
                                            location.kind === "workflow_output"
                                          ? `${location.kind}:${location.port}`
                                          : undefined,
                            },
                          )}

                          label={usageDescription(location)}
                        />
                      </li>
                    ))}
                </ul>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}
      {result && result.users.length < result.total ? (
        <ActionButton
          type="button"
          disabled={loading}
          onClick={async () => {
            setLoading(true);
            setError("");
            try {
              const page = await request("query.configuration_usage", {
                id,
                project_id: projectId,
                offset: result.users.length,
                limit: 100,
                index_version: result.index_version,
              });
              setResult({ ...page, users: [...result.users, ...page.users] });
            } catch (error) {
              setError(String(error));
            } finally {
              setLoading(false);
            }
          }}
        >
          加载更多引用
        </ActionButton>
      ) : null}
      {error ? (
        <p role="alert">
          引用关系读取失败：{displayText(error)}{" "}
          <ActionButton
            type="button"
            disabled={loading}
            onClick={() => {
              setLoading(true);
              setAttempt((value) => value + 1);
            }}
          >
            重试
          </ActionButton>
        </p>
      ) : null}
    </aside>
  );
}
