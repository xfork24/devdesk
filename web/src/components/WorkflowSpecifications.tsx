import { ActionButton } from "./ActionFeedback";
import { JsonField } from "./WorkflowDeliveries";
import {
  latest,
  MarkdownField,
  RevisionSelect,
  type Row,
} from "./configuration-ui";

function SelectorFields({
  value,
  records,
  onChange,
}: {
  value: Row;
  records: Row[];
  onChange: (value: Row) => void;
}) {
  const roles = latest(records.filter((row) => row.kind === "role")).map(
    (row) => ({ ...row, id: row.id }),
  );
  return (
    <>
      <RevisionSelect
        label="匹配 Role（可选）"
        records={roles}
        value={value.role_ref || ""}
        onChange={(role_ref) =>
          onChange({ ...value, role_ref: role_ref || null })
        }
      />
      <RevisionSelect
        label="匹配交付类型（可选）"
        records={records.filter((row) => row.kind === "delivery_type")}
        value={value.delivery_ref || ""}
        onChange={(delivery_ref) =>
          onChange({ ...value, delivery_ref: delivery_ref || null })
        }
      />
      <label>
        必须具有的步骤标签
        <input
          value={(value.tags || []).join(", ")}
          onChange={(event) =>
            onChange({
              ...value,
              tags: event.target.value
                .split(/[,，\n]/)
                .map((value) => value.trim())
                .filter(Boolean),
            })
          }
        />
      </label>
    </>
  );
}

export function WorkflowSpecifications({
  kind,
  data,
  records,
  onChange,
}: {
  kind: string;
  data: Row;
  records: Row[];
  onChange: (value: Row) => void;
}) {
  if (kind === "delivery_type")
    return (
      <div className="full-width">
        <label>
          <input
            type="checkbox"
            checked={
              data.content_schema !== null && data.content_schema !== undefined
            }
            onChange={(event) =>
              onChange({
                content_schema: event.target.checked
                  ? { type: "object" }
                  : null,
              })
            }
          />
          校验结构化内容
        </label>
        {data.content_schema !== null && data.content_schema !== undefined ? (
          <JsonField
            label="内容 JSON Schema"
            value={data.content_schema}
            onChange={(content_schema) => onChange({ content_schema })}
          />
        ) : null}
      </div>
    );
  if (kind === "spec")
    return (
      <div className="full-width">
        <p>
          内容要求由下面的检查项定义，可由绑定的 Agent 按当前 Skill 和材料评审。
        </p>
        <p className="muted">
          文档与规范的关联在 Delivery 中选择，场景覆盖要求在 Standard 中选择。
        </p>
        {(data.checks || []).map((check: Row, index: number) => {
          const patch = (patch: Row) =>
            onChange({
              checks: data.checks.map((value: Row, i: number) =>
                i === index ? { ...value, ...patch } : value,
              ),
            });
          return (
            <fieldset key={index}>
              <legend>检查项 {index + 1}</legend>
              <label>
                检查标识
                <input
                  required
                  value={check.key}
                  onChange={(event) => patch({ key: event.target.value })}
                />
              </label>
              <label>
                检查名称
                <input
                  required
                  value={check.name}
                  onChange={(event) => patch({ name: event.target.value })}
                />
              </label>
              <MarkdownField
                label={`检查要求 ${index + 1}`}
                value={check.instructions}
                required
                onChange={(instructions) => patch({ instructions })}
              />
              <ActionButton
                type="button"
                onClick={() =>
                  onChange({
                    checks: data.checks.filter(
                      (_: Row, i: number) => i !== index,
                    ),
                  })
                }
              >
                删除检查项
              </ActionButton>
            </fieldset>
          );
        })}
        <ActionButton
          type="button"
          onClick={() =>
            onChange({
              checks: [
                ...(data.checks || []),
                {
                  key: `check_${crypto.randomUUID().slice(0, 8)}`,
                  name: "",
                  instructions: "",
                },
              ],
            })
          }
        >
          添加检查项
        </ActionButton>
      </div>
    );
  return (
    <div className="full-width">
      {(data.baseline_rules || []).map((rule: Row, index: number) => {
        const patch = (patch: Row) =>
          onChange({
            baseline_rules: data.baseline_rules.map((value: Row, i: number) =>
              i === index ? { ...value, ...patch } : value,
            ),
          });
        return (
          <fieldset key={index}>
            <legend>覆盖规则 {index + 1}</legend>
            <SelectorFields
              value={rule.selector || {}}
              records={records}
              onChange={(selector) => patch({ selector })}
            />
            <RevisionSelect
              label={`规则 ${index + 1} 必需 Spec`}
              records={records.filter((row) => row.kind === "spec")}
              multiple
              required
              value={rule.required_spec_refs || []}
              onChange={(required_spec_refs) => patch({ required_spec_refs })}
            />
            <label>
              最低匹配步骤数
              <input
                type="number"
                min={1}
                required
                value={rule.min_matches}
                onChange={(event) =>
                  patch({ min_matches: Number(event.target.value) })
                }
              />
            </label>
            <ActionButton
              type="button"
              onClick={() =>
                onChange({
                  baseline_rules: data.baseline_rules.filter(
                    (_: Row, i: number) => i !== index,
                  ),
                })
              }
            >
              删除覆盖规则
            </ActionButton>
          </fieldset>
        );
      })}
      <ActionButton
        type="button"
        onClick={() =>
          onChange({
            baseline_rules: [
              ...(data.baseline_rules || []),
              { selector: {}, required_spec_refs: [], min_matches: 1 },
            ],
          })
        }
      >
        添加覆盖规则
      </ActionButton>
    </div>
  );
}
