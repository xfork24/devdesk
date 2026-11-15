import { ActionButton } from "./ActionFeedback";
import { ResourceEditor } from "./AgentCatalog";
import { Integrations } from "./Integrations";
import { StandardManager } from "./StandardManager";
import { WorkflowManager } from "./WorkflowManager";
import { PermissionResources } from "./PermissionResources";
import { PermissionTargetBinding } from "./PermissionTargetBinding";
import { useGroupTab } from "./configuration-state";
import { type Request, type Row } from "./rpc-types";

type Props = {
  request: Request;
  project?: Row;
  installations: Row[];
  refresh: () => Promise<void>;
};
const processTabs = [
  ["workflow", "Workflow"],
  ["delivery_type", "Delivery"],
  ["spec", "Spec"],
  ["standard", "Standard"],
];
const foundationTabs = [
  ["model", "Model"],
  ["harness", "Harness"],
  ["sandbox", "Sandbox"],
  ["permission", "Permission"],
  ["mcp", "MCP"],
  ["skills", "Skills"],
  ["http", "Tools"],
  ["native", "Harness原生工具"],
];

export function ConfigurationPage({
  group,
  ...props
}: Props & { group: "processes" | "foundations" }) {
  const tabs = group === "processes" ? processTabs : foundationTabs;
  const [tab, choose] = useGroupTab(
    group,
    tabs.map(([id]) => id),
  );
  return (
    <>
      <section className="panel">
        <div className="panel-heading">
          <h2>{group === "processes" ? "流程与规范" : "基础组件"}</h2>
        </div>
        <div
          className="configuration-tabs"
          role="tablist"
          aria-label="配置类别"
        >
          {tabs.map(([id, name]) => (
            <ActionButton
              key={id}
              role="tab"
              aria-selected={tab === id}
              onClick={() => {
                choose(id);
                if (id !== "workflow") {
                  const params = new URLSearchParams(location.search);
                  params.delete("workflow_id");
                  history.replaceState(null, "", `?${params}`);
                }
              }}
            >
              {name}
            </ActionButton>
          ))}
        </div>
      </section>
      {tab === "workflow" ? (
        <WorkflowManager
          key={`workflow:${props.project?.id}`}
          request={props.request}
          project={props.project}
          refresh={props.refresh}
        />
      ) : tab === "standard" ? (
        <StandardManager request={props.request} project={props.project} />
      ) : tab === "permission" ? (
        <>
          <PermissionResources
            request={props.request}
            projectId={props.project?.id}
            refresh={props.refresh}
          />
        </>
      ) : ["model", "harness", "sandbox", "spec", "delivery_type"].includes(
          tab,
        ) ? (
        <ResourceEditor
          key={`${tab}:${props.project?.id}`}
          kind={tab}
          request={props.request}
          project={props.project}
          installations={props.installations}
          revision={0}
          detailFooter={
            tab === "harness" || tab === "sandbox"
              ? (record) =>
                  record ? (
                    <PermissionTargetBinding
                      request={props.request}
                      projectId={props.project?.id}
                      kind={tab}
                      targetRef={record.id}
                    />
                  ) : (
                    <p className="muted">
                      保存此组件后，可为确切修订绑定 Permission。
                    </p>
                  )
              : undefined
          }
        />
      ) : (
        <Integrations
          key={`${tab}:${props.project?.id}`}
          request={props.request}
          project={props.project}
          installations={props.installations}
          kind={tab}
          contentOnly
        />
      )}
    </>
  );
}
